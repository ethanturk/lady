use lady_git::{CommitOpts, DiffSpec, GitAuth, GitEngine, GixEngine, GraphQuery, MergeOpts};
use lady_graph::layout_continuation;
use lady_proto::{
    ApplyOutcome, Blame, CommitMeta, FfMode, FileDiff, GitHubAccount, GitIdentity, MergeOutcome,
    Oid, RebaseOutcome, RefInfo, RepoAuth, RepoId, RepoSettings, RepositoryFamily, ResetMode,
    WorkingTree,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use tauri::State;

mod ai;
// The updater plugin (and its commands) is desktop-only — it has no mobile
// implementation, so the whole module is gated off on iOS/Android.
#[cfg(desktop)]
mod updater;
mod watcher;

/// OS-keychain service name for all of Lady's hosting/transport tokens
/// (single-account legacy keys and per-account `github-token:<id>` keys alike).
/// The credential-helper subprocess uses the same service to read them back.
const KEYCHAIN_SERVICE: &str = "Lady-Hosting";
static SETTINGS_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// Parameters for the walk_log command (mirrors GraphQuery for the bridge).
#[derive(Deserialize)]
pub struct WalkLogQuery {
    pub start: Option<String>,
    pub limit: usize,
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    let pkg = app.package_info();
    AppInfo {
        name: pkg.name.clone(),
        version: pkg.version.to_string(),
    }
}

#[tauri::command]
fn open_repo(path: String, engine: State<GixEngine>) -> Result<RepoId, String> {
    engine
        .open(std::path::Path::new(&path))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_refs(repo: RepoId, engine: State<GixEngine>) -> Result<Vec<RefInfo>, String> {
    engine.list_refs(&repo).map_err(|e| e.to_string())
}

#[tauri::command]
async fn walk_log(
    repo: RepoId,
    query: WalkLogQuery,
    engine: State<'_, GixEngine>,
) -> Result<Vec<CommitMeta>, String> {
    let gq = GraphQuery {
        start: query.start.map(Oid::from),
        limit: query.limit,
    };
    engine.walk_log(&repo, gq).map_err(|e| e.to_string())
}

/// A single line segment for the canvas graph renderer.
#[derive(Serialize)]
pub struct EdgeData {
    pub from_lane: usize,
    pub to_lane: usize,
}

/// Combined commit metadata + graph layout row, ready for the hybrid renderer.
#[derive(Serialize)]
pub struct CommitGraphRow {
    pub oid: String,
    pub parents: Vec<String>,
    pub author_name: String,
    pub summary: String,
    pub time: i64,
    pub lane: usize,
    pub num_lanes: usize,
    pub edges: Vec<EdgeData>,
    pub refs: Vec<String>,
}

/// Result of walk_log_graph — rows plus the opaque lane state for the next page.
#[derive(Serialize)]
pub struct WalkLogGraphResult {
    pub rows: Vec<CommitGraphRow>,
    /// Serialized ActiveLanes state; pass back as `layout_state` for the next page.
    pub layout_state: Vec<Option<String>>,
}

#[tauri::command]
async fn walk_log_graph(
    repo: RepoId,
    query: WalkLogQuery,
    layout_state: Option<Vec<Option<String>>>,
    engine: State<'_, GixEngine>,
) -> Result<WalkLogGraphResult, String> {
    let gq = GraphQuery {
        start: query.start.map(Oid::from),
        limit: query.limit,
    };
    let commits = engine.walk_log(&repo, gq).map_err(|e| e.to_string())?;

    // Build an oid → ref-name map for branch/tag/HEAD labels on graph rows.
    // Tags are prefixed with `tag:` so the renderer can style them. HEAD is
    // prefixed with `head:` (e.g. `head:main`) so the current branch chip gets a
    // checkmark. When HEAD points at a named branch, skip the duplicate branch
    // entry so we only show one chip.
    let refs = engine.list_refs(&repo).map_err(|e| e.to_string())?;
    let mut head_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for r in &refs {
        if r.kind == lady_proto::RefKind::Head {
            head_names.insert(r.name.clone());
        }
    }
    let mut refs_by_oid: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in refs {
        let label = match r.kind {
            lady_proto::RefKind::Tag => format!("tag:{}", r.name),
            lady_proto::RefKind::Head => format!("head:{}", r.name),
            // Skip branch/tag/remote entries whose name matches the current HEAD
            // branch — the `head:<name>` entry already covers them.
            _ if head_names.contains(&r.name) => continue,
            _ => r.name,
        };
        refs_by_oid
            .entry(r.target.as_str().to_owned())
            .or_default()
            .push(label);
    }

    // Deserialize the opaque lane state (Option<String> → Option<Oid>).
    let state: Vec<Option<Oid>> = layout_state
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.map(Oid::from))
        .collect();

    let (graph_rows, next_state) = layout_continuation(&commits, state);

    let rows = commits
        .into_iter()
        .zip(graph_rows)
        .map(|(c, r)| CommitGraphRow {
            oid: c.oid.as_str().to_owned(),
            parents: c.parents.iter().map(|p| p.as_str().to_owned()).collect(),
            author_name: c.author.name,
            summary: c.summary,
            time: c.time,
            lane: r.lane,
            num_lanes: r.num_lanes,
            edges: r
                .edges
                .into_iter()
                .map(|e| EdgeData {
                    from_lane: e.from_lane,
                    to_lane: e.to_lane,
                })
                .collect(),
            refs: refs_by_oid.get(c.oid.as_str()).cloned().unwrap_or_default(),
        })
        .collect();

    let layout_state_out = next_state
        .into_iter()
        .map(|opt| opt.map(|oid| oid.as_str().to_owned()))
        .collect();

    Ok(WalkLogGraphResult {
        rows,
        layout_state: layout_state_out,
    })
}

// ============================================================================
// Diff & Blame Commands
// ============================================================================

#[tauri::command]
async fn diff(
    repo: RepoId,
    commit: String,
    engine: State<'_, GixEngine>,
) -> Result<Vec<FileDiff>, String> {
    let oid = Oid::from(commit);
    engine.diff_commit(&repo, &oid).map_err(|e| e.to_string())
}

/// Bridge DTO for a [`DiffSpec`]: `kind` selects the variant and `value` is the
/// commit oid (Commit) or file path (WorkingVsIndex / IndexVsHead).
#[derive(Deserialize)]
pub struct DiffSpecArg {
    pub kind: String,
    pub value: String,
}

#[tauri::command]
async fn diff_spec(
    repo: RepoId,
    spec: DiffSpecArg,
    engine: State<'_, GixEngine>,
) -> Result<Vec<FileDiff>, String> {
    let spec = match spec.kind.as_str() {
        "Commit" => DiffSpec::Commit(Oid::from(spec.value)),
        "WorkingVsIndex" => DiffSpec::WorkingVsIndex(spec.value),
        "IndexVsHead" => DiffSpec::IndexVsHead(spec.value),
        other => return Err(format!("unknown DiffSpec kind: {other}")),
    };
    engine.diff_spec(&repo, &spec).map_err(|e| e.to_string())
}

#[tauri::command]
async fn blame(
    repo: RepoId,
    path: String,
    at: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<Blame, String> {
    let at = at.map(Oid::from);
    engine
        .blame(&repo, &path, at.as_ref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn file_history(
    repo: RepoId,
    path: String,
    engine: State<'_, GixEngine>,
) -> Result<Vec<CommitMeta>, String> {
    engine.file_history(&repo, &path).map_err(|e| e.to_string())
}

// ============================================================================
// Working Tree Commands
// ============================================================================

/// Whether a repo's worktree has uncommitted changes (drives the tab star).
#[tauri::command]
fn repo_dirty(repo: RepoId, engine: State<GixEngine>) -> Result<bool, String> {
    engine.is_dirty(&repo).map_err(|e| e.to_string())
}

/// All tracked file paths at HEAD (drives the command palette's file search).
#[tauri::command]
fn list_files(repo: RepoId, engine: State<GixEngine>) -> Result<Vec<String>, String> {
    engine.list_files(&repo).map_err(|e| e.to_string())
}

/// Working-tree status (staged / unstaged / untracked) for the Changes view.
#[tauri::command]
async fn status(repo: RepoId, engine: State<'_, GixEngine>) -> Result<WorkingTree, String> {
    engine.status(&repo).map_err(|e| e.to_string())
}

/// Stage whole files into the index.
#[tauri::command]
fn stage_paths(repo: RepoId, paths: Vec<String>, engine: State<GixEngine>) -> Result<(), String> {
    engine.stage_paths(&repo, &paths).map_err(|e| e.to_string())
}

/// Unstage whole files from the index.
#[tauri::command]
fn unstage_paths(repo: RepoId, paths: Vec<String>, engine: State<GixEngine>) -> Result<(), String> {
    engine
        .unstage_paths(&repo, &paths)
        .map_err(|e| e.to_string())
}

/// Stage a subset of `hunks` (indices into the unstaged working-vs-index diff)
/// of `path` by building a patch and `git apply --cached`-ing it.
#[tauri::command]
fn stage_hunks(
    repo: RepoId,
    path: String,
    hunks: Vec<usize>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    let diffs = engine
        .diff_spec(&repo, &DiffSpec::WorkingVsIndex(path.clone()))
        .map_err(|e| e.to_string())?;
    let Some(file) = diffs.into_iter().next() else {
        return Ok(());
    };
    let patch = lady_diff::build_patch(&path, &file.hunks, &hunks);
    engine
        .apply_patch(&repo, &patch, false, true)
        .map_err(|e| e.to_string())
}

/// Unstage a subset of `hunks` (indices into the staged index-vs-HEAD diff) of
/// `path` by reverse-applying the patch against the index.
#[tauri::command]
fn unstage_hunks(
    repo: RepoId,
    path: String,
    hunks: Vec<usize>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    let diffs = engine
        .diff_spec(&repo, &DiffSpec::IndexVsHead(path.clone()))
        .map_err(|e| e.to_string())?;
    let Some(file) = diffs.into_iter().next() else {
        return Ok(());
    };
    let patch = lady_diff::build_patch(&path, &file.hunks, &hunks);
    engine
        .apply_patch(&repo, &patch, true, true)
        .map_err(|e| e.to_string())
}

/// Stage selected lines (`lines` = changed-line indices within hunk `hunk`) of
/// `path` from its unstaged working-vs-index diff.
#[tauri::command]
fn stage_lines(
    repo: RepoId,
    path: String,
    hunk: usize,
    lines: Vec<usize>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    let diffs = engine
        .diff_spec(&repo, &DiffSpec::WorkingVsIndex(path.clone()))
        .map_err(|e| e.to_string())?;
    let Some(file) = diffs.into_iter().next() else {
        return Ok(());
    };
    let sel = vec![lady_diff::LineSel { hunk, lines }];
    let patch = lady_diff::build_patch_lines(&path, &file.hunks, &sel);
    engine
        .apply_patch(&repo, &patch, false, true)
        .map_err(|e| e.to_string())
}

/// Discard whole unstaged `hunks` of `path` from the working tree
/// (DESTRUCTIVE — reverse-applies the working diff, no `--cached`).
#[tauri::command]
fn discard_hunks(
    repo: RepoId,
    path: String,
    hunks: Vec<usize>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    let diffs = engine
        .diff_spec(&repo, &DiffSpec::WorkingVsIndex(path.clone()))
        .map_err(|e| e.to_string())?;
    let Some(file) = diffs.into_iter().next() else {
        return Ok(());
    };
    let patch = lady_diff::build_patch(&path, &file.hunks, &hunks);
    engine
        .apply_patch(&repo, &patch, true, false)
        .map_err(|e| e.to_string())
}

/// Discard selected unstaged `lines` of hunk `hunk` of `path` from the working
/// tree (DESTRUCTIVE — reverse-applies a line-level patch, no `--cached`).
#[tauri::command]
fn discard_lines(
    repo: RepoId,
    path: String,
    hunk: usize,
    lines: Vec<usize>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    let diffs = engine
        .diff_spec(&repo, &DiffSpec::WorkingVsIndex(path.clone()))
        .map_err(|e| e.to_string())?;
    let Some(file) = diffs.into_iter().next() else {
        return Ok(());
    };
    let sel = vec![lady_diff::LineSel { hunk, lines }];
    let patch = lady_diff::build_patch_lines(&path, &file.hunks, &sel);
    engine
        .apply_patch(&repo, &patch, true, false)
        .map_err(|e| e.to_string())
}

/// Delete untracked `paths` from the working tree (DESTRUCTIVE).
#[tauri::command]
fn discard_untracked(
    repo: RepoId,
    paths: Vec<String>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .discard_untracked(&repo, &paths)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Commit Commands
// ============================================================================

/// Commit the staged changes, or amend the tip when `amend` is set. Returns the
/// new commit Oid.
#[tauri::command]
async fn commit(
    repo: RepoId,
    message: String,
    amend: bool,
    sign: bool,
    app: tauri::AppHandle,
) -> Result<Oid, String> {
    use tauri::{Emitter, Manager};
    // Run the (potentially slow) commit + pre-commit hooks on a blocking thread
    // so the UI stays responsive; relay each hook output line to the frontend
    // over `commit-progress` for live feedback.
    tauri::async_runtime::spawn_blocking(move || {
        let engine = app.state::<GixEngine>();
        let emit_app = app.clone();
        let mut on_line = move |line: &str| {
            let _ = emit_app.emit("commit-progress", line.to_string());
        };
        engine
            .commit_streaming(&repo, &message, &CommitOpts { amend, sign }, &mut on_line)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("commit task failed: {e}"))?
}

/// Signature verification status for each commit oid (PH3-005 badge data).
#[tauri::command]
async fn signature_statuses(
    repo: RepoId,
    oids: Vec<String>,
    engine: State<'_, GixEngine>,
) -> Result<Vec<lady_proto::SignatureStatus>, String> {
    let oids: Vec<Oid> = oids.into_iter().map(Oid::from).collect();
    engine
        .signature_statuses(&repo, &oids)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Worktree Commands
// ============================================================================

/// List the repository's worktrees (PH3-006).
#[tauri::command]
fn list_worktrees(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<Vec<lady_proto::Worktree>, String> {
    engine.list_worktrees(&repo).map_err(|e| e.to_string())
}

/// Backend-owned repository-family summary (ADR-0012).
#[tauri::command]
fn repository_family(repo: RepoId, engine: State<GixEngine>) -> Result<RepositoryFamily, String> {
    engine.repository_family(&repo).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct RepositoryFamilyIdentity {
    pub id: String,
    pub main_path: String,
}

/// Cheap family identity for opening/switching: avoids full worktree status
/// enrichment so the UI can activate the selected checkout quickly.
#[tauri::command]
fn repository_family_identity(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<RepositoryFamilyIdentity, String> {
    let id = engine
        .repository_family_id(&repo)
        .map_err(|e| e.to_string())?;
    let main_path = std::path::Path::new(id.as_str())
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| id.as_str().to_string());
    Ok(RepositoryFamilyIdentity {
        id: id.as_str().to_string(),
        main_path,
    })
}

/// Add a worktree at `path`; create branch `branch` there when `new_branch`.
#[tauri::command]
fn add_worktree(
    repo: RepoId,
    path: String,
    branch: Option<String>,
    new_branch: bool,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .add_worktree(&repo, &path, branch.as_deref(), new_branch)
        .map_err(|e| e.to_string())
}

/// Remove the worktree at `path`.
#[tauri::command]
fn remove_worktree(repo: RepoId, path: String, engine: State<GixEngine>) -> Result<(), String> {
    engine
        .remove_worktree(&repo, &path)
        .map_err(|e| e.to_string())
}

/// Prune stale worktree entries.
#[tauri::command]
fn prune_worktrees(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    engine.prune_worktrees(&repo).map_err(|e| e.to_string())
}

/// List submodules with status (PH4-009).
#[tauri::command]
fn list_submodules(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<Vec<lady_proto::Submodule>, String> {
    engine.list_submodules(&repo).map_err(|e| e.to_string())
}

/// Add a submodule at `path` from `url`.
#[tauri::command]
fn add_submodule(
    repo: RepoId,
    url: String,
    path: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .add_submodule(&repo, &url, &path)
        .map_err(|e| e.to_string())
}

/// Initialize + check out all submodules.
#[tauri::command]
async fn init_submodules(repo: RepoId, engine: State<'_, GixEngine>) -> Result<(), String> {
    engine.init_submodules(&repo).map_err(|e| e.to_string())
}

/// Update submodules to their pinned commits.
#[tauri::command]
async fn update_submodules(repo: RepoId, engine: State<'_, GixEngine>) -> Result<(), String> {
    engine.update_submodules(&repo).map_err(|e| e.to_string())
}

/// Sync submodule URLs from `.gitmodules`.
#[tauri::command]
async fn sync_submodules(repo: RepoId, engine: State<'_, GixEngine>) -> Result<(), String> {
    engine.sync_submodules(&repo).map_err(|e| e.to_string())
}

/// Deinitialize the submodule at `path`.
#[tauri::command]
fn deinit_submodule(repo: RepoId, path: String, engine: State<GixEngine>) -> Result<(), String> {
    engine
        .deinit_submodule(&repo, &path)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Flow Commands (git-flow integration)
// ============================================================================

/// Read the persisted git-flow config (PH4-008).
#[tauri::command]
fn flow_config(repo: RepoId, engine: State<GixEngine>) -> Result<lady_proto::FlowConfig, String> {
    engine.flow_config(&repo).map_err(|e| e.to_string())
}

/// Initialize git-flow with `config`.
#[tauri::command]
fn flow_init(
    repo: RepoId,
    config: lady_proto::FlowConfig,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine.flow_init(&repo, &config).map_err(|e| e.to_string())
}

/// Start a flow branch of `kind` named `name`; returns the branch name.
#[tauri::command]
fn flow_start(
    repo: RepoId,
    kind: lady_proto::FlowKind,
    name: String,
    engine: State<GixEngine>,
) -> Result<String, String> {
    engine
        .flow_start(&repo, kind, &name)
        .map_err(|e| e.to_string())
}

/// Finish a flow branch of `kind` named `name`.
#[tauri::command]
fn flow_finish(
    repo: RepoId,
    kind: lady_proto::FlowKind,
    name: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .flow_finish(&repo, kind, &name)
        .map_err(|e| e.to_string())
}

// ============================================================================
// LFS Commands (Git Large File Storage)
// ============================================================================

/// Git LFS status for a repo: availability, tracked patterns, files (PH4-007).
#[tauri::command]
fn lfs_status(repo: RepoId, engine: State<GixEngine>) -> Result<lady_proto::LfsStatus, String> {
    engine.lfs_status(&repo).map_err(|e| e.to_string())
}

/// Track `pattern` with Git LFS (`git lfs track`).
#[tauri::command]
fn lfs_track(repo: RepoId, pattern: String, engine: State<GixEngine>) -> Result<(), String> {
    engine.lfs_track(&repo, &pattern).map_err(|e| e.to_string())
}

// ============================================================================
// Reflog & Bisect Commands
// ============================================================================

/// The reflog for `refname` (default HEAD), newest first (PH3-007).
#[tauri::command]
async fn reflog(
    repo: RepoId,
    refname: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<Vec<lady_proto::ReflogEntry>, String> {
    let refname = refname.unwrap_or_else(|| "HEAD".to_string());
    engine.reflog(&repo, &refname).map_err(|e| e.to_string())
}

/// Start a bisect bounded by `bad` and `good` (PH3-008).
#[tauri::command]
fn bisect_start(
    repo: RepoId,
    bad: String,
    good: String,
    engine: State<GixEngine>,
) -> Result<lady_proto::BisectState, String> {
    engine
        .bisect_start(&repo, &Oid::from(bad), &Oid::from(good))
        .map_err(|e| e.to_string())
}

/// Mark the current bisect commit `good` / `bad` / `skip`.
#[tauri::command]
fn bisect_mark(
    repo: RepoId,
    mark: String,
    engine: State<GixEngine>,
) -> Result<lady_proto::BisectState, String> {
    engine.bisect_mark(&repo, &mark).map_err(|e| e.to_string())
}

/// Exit bisect, restoring the original branch.
#[tauri::command]
fn bisect_reset(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    engine.bisect_reset(&repo).map_err(|e| e.to_string())
}

/// The current bisect state (empty when not bisecting).
#[tauri::command]
fn bisect_state(repo: RepoId, engine: State<GixEngine>) -> Result<lady_proto::BisectState, String> {
    engine.bisect_state(&repo).map_err(|e| e.to_string())
}

/// Parse the typed placeholders out of a custom-command template (PH3-009).
#[tauri::command]
fn parse_placeholders(template: String) -> Vec<lady_proto::Placeholder> {
    lady_git::custom::parse_placeholders(&template)
}

// ============================================================================
// Custom Commands & External Tools
// ============================================================================

/// Run a custom command: substitute `values` into `template` to build a safe
/// argv, then execute it against the repo. Returns stdout/stderr/exit code.
#[tauri::command]
async fn run_custom_command(
    repo: RepoId,
    template: String,
    values: std::collections::HashMap<String, String>,
    engine: State<'_, GixEngine>,
) -> Result<lady_proto::CommandOutput, String> {
    let argv = lady_git::custom::build_argv(&template, &values);
    engine.run_custom(&repo, &argv).map_err(|e| e.to_string())
}

/// Launch the configured external diff tool on `path` (PH3-010). Async because
/// `git difftool` blocks until the external (GUI) tool is closed — running it on
/// the main thread would freeze the whole app for the tool's lifetime.
#[tauri::command]
async fn launch_difftool(
    repo: RepoId,
    path: String,
    commit: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    engine
        .launch_difftool(&repo, &path, commit.as_deref())
        .map_err(|e| e.to_string())
}

/// Launch the configured external merge tool on a conflicted `path` (PH3-010).
/// Async for the same reason as [`launch_difftool`] — `git mergetool` blocks
/// until the tool exits.
#[tauri::command]
async fn launch_mergetool(
    repo: RepoId,
    path: String,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    engine
        .launch_mergetool(&repo, &path)
        .map_err(|e| e.to_string())
}

/// The most recent commit subjects (newest first), capped at `limit`.
#[tauri::command]
fn recent_messages(
    repo: RepoId,
    limit: usize,
    engine: State<GixEngine>,
) -> Result<Vec<String>, String> {
    engine
        .recent_messages(&repo, limit)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Branch & Tag Commands
// ============================================================================

/// Create branch `name` at `start_point` (or HEAD when omitted).
#[tauri::command]
fn create_branch(
    repo: RepoId,
    name: String,
    start_point: Option<String>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .create_branch(&repo, &name, start_point.as_deref())
        .map_err(|e| e.to_string())
}

/// Delete branch `name`; `force` deletes an unmerged branch.
#[tauri::command]
fn delete_branch(
    repo: RepoId,
    name: String,
    force: bool,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .delete_branch(&repo, &name, force)
        .map_err(|e| e.to_string())
}

/// Check out `target` (branch or revision); `force` overwrites local changes.
#[tauri::command]
fn checkout(
    repo: RepoId,
    target: String,
    force: bool,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .checkout(&repo, &target, force)
        .map_err(|e| e.to_string())
}

/// Create tag `name` at `target` (or HEAD); annotated when `message` is set.
#[tauri::command]
fn create_tag(
    repo: RepoId,
    name: String,
    target: Option<String>,
    message: Option<String>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .create_tag(&repo, &name, target.as_deref(), message.as_deref())
        .map_err(|e| e.to_string())
}

/// Delete tag `name`.
#[tauri::command]
fn delete_tag(repo: RepoId, name: String, engine: State<GixEngine>) -> Result<(), String> {
    engine.delete_tag(&repo, &name).map_err(|e| e.to_string())
}

/// Move an existing lightweight tag `name` to `target`. This is the local
/// equivalent of "fast-forwarding" a tag to a newer commit (`git tag -f`).
/// Annotated tags are recreated as lightweight tags at the new target.
#[tauri::command]
fn move_tag(
    repo: RepoId,
    name: String,
    target: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .move_tag(&repo, &name, &target)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Remote & Clone Commands
// ============================================================================

#[tauri::command]
fn reset(
    repo: RepoId,
    target: String,
    mode: ResetMode,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .reset(&repo, &Oid::from(target), mode)
        .map_err(|e| e.to_string())
}

/// Clone `url` into `dest` via system git (ADR-0003 shell-out tier), streaming
/// git's progress lines to the frontend as `clone-progress` events, and open
/// the result.
#[tauri::command]
async fn clone_repo(
    url: String,
    dest: String,
    account: Option<String>,
    app: tauri::AppHandle,
    engine: State<'_, GixEngine>,
) -> Result<RepoId, String> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use tauri::Emitter;

    // When a GitHub account is chosen, clone with its credentials (the repo isn't
    // open yet, so auth is selected explicitly rather than from a stored override).
    let auth = account
        .map(|id| {
            let login = load_settings_inner()
                .github_accounts
                .into_iter()
                .find(|a| a.id == id)
                .map(|a| a.login)
                .unwrap_or_else(|| id.clone());
            https_account_git_auth(&id, &login)
        })
        .unwrap_or_else(GitAuth::none);

    let mut cmd = Command::new("git");
    // Never block on a terminal credential prompt (no TTY behind the GUI) — fail
    // fast with a clear auth error instead of "could not read Username … Device
    // not configured". Auth comes from `auth` below or a configured helper.
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never");
    for (k, v) in &auth.config {
        cmd.arg("-c").arg(format!("{k}={v}"));
    }
    cmd.args(["clone", "--progress", &url, &dest]);
    for (k, v) in &auth.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start git clone: {e}"))?;

    // git writes progress to stderr; relay each line as an event.
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = app.emit("clone-progress", line);
        }
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("git clone failed ({status})"));
    }

    engine
        .open(std::path::Path::new(&dest))
        .map_err(|e| e.to_string())
}

/// Fetch from `remote` (default when `None`), streaming git progress to the
/// frontend as `fetch-progress` events. Uses the hosting PAT for HTTPS when
/// connected in Settings; otherwise falls back to system git credentials.
#[tauri::command]
async fn fetch(
    repo: RepoId,
    remote: Option<String>,
    app: tauri::AppHandle,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<(), String> {
    use tauri::Emitter;
    let mut auth = git_auth_for_repo(&repo, &engine);
    if auth.is_empty() {
        if let Some(token) = http_bearer_for_remote(&repo, remote.as_deref(), &engine, &hosting) {
            auth = http_bearer_git_auth(token);
        }
    }
    let mut emit = |line: &str| {
        let _ = app.emit("fetch-progress", line.to_string());
    };
    engine
        .fetch(&repo, remote.as_deref(), &auth, &mut emit)
        .map_err(friendly_git_err)
}

/// Quiet best-effort background fetch used by the UI poller. It uses the same
/// credential resolution as the explicit Fetch action but does not emit progress
/// events, so periodic refreshes do not disturb the toolbar status line.
#[tauri::command]
async fn fetch_background(
    repo: RepoId,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<(), String> {
    let mut auth = git_auth_for_repo(&repo, &engine);
    if auth.is_empty() {
        if let Some(token) = http_bearer_for_remote(&repo, None, &engine, &hosting) {
            auth = http_bearer_git_auth(token);
        }
    }
    let mut ignore = |_line: &str| {};
    engine
        .fetch(&repo, None, &auth, &mut ignore)
        .map_err(friendly_git_err)
}

/// Start watching `repo` so working-tree / `.git` changes emit `repo-fs-changed`
/// (the live-refresh signal that replaces interval polling). Replaces any
/// existing watcher for the repo; returns an error on platforms without a
/// watcher so the frontend can fall back to polling.
// Async so the watch registration (which can block while the OS walks the tree
// to install per-directory watches) runs off the main thread — a synchronous
// command would freeze the UI for seconds on a large repo.
#[tauri::command]
async fn watch_repo(
    repo: RepoId,
    app: tauri::AppHandle,
    engine: State<'_, GixEngine>,
    watchers: State<'_, watcher::RepoWatchers>,
) -> Result<(), String> {
    let workdir = engine.workdir_path(&repo).map_err(|e| e.to_string())?;
    let common = engine.git_common_dir(&repo).map_err(|e| e.to_string())?;
    watcher::watch(&watchers, repo, workdir, common, app)
}

/// Stop watching `repo` (idempotent). Async because dropping the watcher handle
/// joins its debounce thread — keep that off the main thread so a tab switch
/// never stalls the UI.
#[tauri::command]
async fn unwatch_repo(
    repo: RepoId,
    watchers: State<'_, watcher::RepoWatchers>,
) -> Result<(), String> {
    watcher::unwatch(&watchers, &repo);
    Ok(())
}

/// Pull (fetch + integrate) from `remote`/`branch`, or the configured upstream.
/// Progress streams as `fetch-progress` events.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn pull(
    repo: RepoId,
    remote: Option<String>,
    branch: Option<String>,
    strategy: Option<String>,
    remember: Option<bool>,
    app: tauri::AppHandle,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<(), String> {
    use tauri::Emitter;
    // Persist the chosen reconcile strategy first when asked, so the user isn't
    // re-prompted on every diverged pull.
    if remember.unwrap_or(false) {
        if let Some(s) = strategy.as_deref() {
            engine
                .set_pull_reconcile(&repo, s)
                .map_err(friendly_git_err)?;
        }
    }
    let mut auth = git_auth_for_repo(&repo, &engine);
    if auth.is_empty() {
        if let Some(token) = http_bearer_for_remote(&repo, remote.as_deref(), &engine, &hosting) {
            auth = http_bearer_git_auth(token);
        }
    }
    let mut emit = |line: &str| {
        let _ = app.emit("fetch-progress", line.to_string());
    };
    engine
        .pull(
            &repo,
            remote.as_deref(),
            branch.as_deref(),
            strategy.as_deref(),
            &auth,
            &mut emit,
        )
        .map_err(friendly_git_err)
}

/// Push the current branch to `remote`/`branch`. `set_upstream` records the
/// tracking ref; `force` allows a non-fast-forward update. Progress streams as
/// `push-progress` events; rejections surface git's message verbatim.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn push(
    repo: RepoId,
    remote: Option<String>,
    branch: Option<String>,
    set_upstream: bool,
    force: bool,
    app: tauri::AppHandle,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<(), String> {
    use tauri::Emitter;
    let mut auth = git_auth_for_repo(&repo, &engine);
    if auth.is_empty() {
        if let Some(token) = http_bearer_for_remote(&repo, remote.as_deref(), &engine, &hosting) {
            auth = http_bearer_git_auth(token);
        }
    }
    let mut emit = |line: &str| {
        let _ = app.emit("push-progress", line.to_string());
    };
    engine
        .push(
            &repo,
            remote.as_deref(),
            branch.as_deref(),
            set_upstream,
            force,
            &auth,
            &mut emit,
        )
        .map_err(friendly_git_err)
}

/// Delete `refspec` on `remote` by pushing an empty source to it
/// (`git push <remote> :<refspec>`). `refspec` is a full ref name such as
/// `refs/tags/v1` or `refs/heads/feature`. Auth mirrors [`push`].
#[tauri::command]
fn delete_remote_ref(
    repo: RepoId,
    remote: String,
    refspec: String,
    app: tauri::AppHandle,
    engine: State<GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<(), String> {
    use tauri::Emitter;
    let mut auth = git_auth_for_repo(&repo, &engine);
    if auth.is_empty() {
        if let Some(token) = http_bearer_for_remote(&repo, Some(&remote), &engine, &hosting) {
            auth = http_bearer_git_auth(token);
        }
    }
    let mut emit = |line: &str| {
        let _ = app.emit("push-progress", line.to_string());
    };
    engine
        .push(
            &repo,
            Some(&remote),
            Some(&format!(":{refspec}")),
            false,
            false,
            &auth,
            &mut emit,
        )
        .map_err(friendly_git_err)
}

/// How far the current branch is ahead/behind its upstream (`None` if untracked).
#[tauri::command]
fn ahead_behind(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<Option<lady_proto::AheadBehind>, String> {
    engine.ahead_behind(&repo).map_err(|e| e.to_string())
}

/// Incoming/outgoing counts for comparable local and remote branch rows.
#[tauri::command]
fn branches_ahead_behind(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<std::collections::BTreeMap<String, lady_proto::AheadBehind>, String> {
    engine
        .branches_ahead_behind(&repo)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn stash_save(
    repo: RepoId,
    message: Option<String>,
    include_untracked: bool,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .stash_save(&repo, message.as_deref(), include_untracked)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn stash_list(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<Vec<lady_proto::StashEntry>, String> {
    engine.stash_list(&repo).map_err(|e| e.to_string())
}

#[tauri::command]
fn stash_apply(repo: RepoId, index: usize, engine: State<GixEngine>) -> Result<(), String> {
    engine.stash_apply(&repo, index).map_err(|e| e.to_string())
}

#[tauri::command]
fn stash_pop(repo: RepoId, index: usize, engine: State<GixEngine>) -> Result<(), String> {
    engine.stash_pop(&repo, index).map_err(|e| e.to_string())
}

#[tauri::command]
fn stash_drop(repo: RepoId, index: usize, engine: State<GixEngine>) -> Result<(), String> {
    engine.stash_drop(&repo, index).map_err(|e| e.to_string())
}

// ============================================================================
// Merge, Rebase & Integration Commands
// ============================================================================

#[tauri::command]
async fn merge(
    repo: RepoId,
    source: String,
    fast_forward: String,
    commit_message: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<MergeOutcome, String> {
    let fast_forward = match fast_forward.as_str() {
        "Auto" => FfMode::Auto,
        "Only" => FfMode::Only,
        "Never" => FfMode::Never,
        other => return Err(format!("unknown fast-forward mode: {other}")),
    };
    let opts = MergeOpts {
        fast_forward,
        commit_message,
    };
    engine
        .merge(&repo, &source, &opts)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn merge_abort(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    engine.merge_abort(&repo).map_err(|e| e.to_string())
}

#[tauri::command]
async fn cherry_pick(
    repo: RepoId,
    oid: String,
    engine: State<'_, GixEngine>,
) -> Result<ApplyOutcome, String> {
    engine
        .cherry_pick(&repo, &Oid::from(oid))
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn revert(
    repo: RepoId,
    oid: String,
    engine: State<'_, GixEngine>,
) -> Result<ApplyOutcome, String> {
    engine
        .revert(&repo, &Oid::from(oid))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn sequencer_abort(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    engine.sequencer_abort(&repo).map_err(|e| e.to_string())
}

#[tauri::command]
async fn rebase(
    repo: RepoId,
    branch: String,
    onto: String,
    engine: State<'_, GixEngine>,
) -> Result<RebaseOutcome, String> {
    engine
        .rebase(&repo, &branch, &onto)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rebase_abort(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    engine.rebase_abort(&repo).map_err(|e| e.to_string())
}

/// List the currently conflicted paths (PH3-001).
#[tauri::command]
fn list_conflicts(repo: RepoId, engine: State<GixEngine>) -> Result<Vec<String>, String> {
    engine.list_conflicts(&repo).map_err(|e| e.to_string())
}

/// The three index-stage sides (base / ours / theirs) of a conflicted file,
/// for the 3-pane resolver.
#[tauri::command]
fn conflict_sides(
    repo: RepoId,
    path: String,
    engine: State<GixEngine>,
) -> Result<lady_proto::ConflictSides, String> {
    engine
        .conflict_sides(&repo, &path)
        .map_err(|e| e.to_string())
}

/// Parse a conflicted file's markers into context + conflict regions.
#[tauri::command]
fn parse_conflict(
    repo: RepoId,
    path: String,
    engine: State<GixEngine>,
) -> Result<lady_proto::ParsedConflict, String> {
    engine
        .parse_conflict(&repo, &path)
        .map_err(|e| e.to_string())
}

/// Resolve a conflicted file by taking our side of every region.
#[tauri::command]
fn take_ours(repo: RepoId, path: String, engine: State<GixEngine>) -> Result<(), String> {
    engine.take_ours(&repo, &path).map_err(|e| e.to_string())
}

/// Resolve a conflicted file by taking their side of every region.
#[tauri::command]
fn take_theirs(repo: RepoId, path: String, engine: State<GixEngine>) -> Result<(), String> {
    engine.take_theirs(&repo, &path).map_err(|e| e.to_string())
}

/// Write the edited result-pane `content` as the resolution of `path`.
#[tauri::command]
fn write_resolution(
    repo: RepoId,
    path: String,
    content: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .write_resolution(&repo, &path, content.as_bytes())
        .map_err(|e| e.to_string())
}

/// Mark `path` resolved (stage it).
#[tauri::command]
fn mark_resolved(repo: RepoId, path: String, engine: State<GixEngine>) -> Result<(), String> {
    engine
        .mark_resolved(&repo, &path)
        .map_err(|e| e.to_string())
}

/// What mid-operation state the repo is in (merge / rebase / cherry-pick / …).
#[tauri::command]
fn conflict_state(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<lady_proto::ConflictState, String> {
    engine.conflict_state(&repo).map_err(|e| e.to_string())
}

/// Abort whatever operation is in progress (routes to the right `--abort`).
#[tauri::command]
fn conflict_abort(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    engine.conflict_abort(&repo).map_err(|e| e.to_string())
}

/// Finish an in-progress merge / cherry-pick / revert once all conflicts are
/// resolved (creates the merge commit or runs the sequencer's `--continue`).
#[tauri::command]
fn conflict_continue(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<lady_proto::ApplyOutcome, String> {
    engine.sequencer_continue(&repo).map_err(|e| e.to_string())
}

/// Run an interactive rebase onto `onto` driven by `plan` (PH3-003).
#[tauri::command]
fn rebase_interactive(
    repo: RepoId,
    onto: String,
    plan: Vec<lady_proto::RebaseStep>,
    engine: State<GixEngine>,
) -> Result<RebaseOutcome, String> {
    engine
        .rebase_interactive(&repo, &onto, &plan)
        .map_err(|e| e.to_string())
}

/// Continue an in-progress (interactive) rebase.
#[tauri::command]
fn rebase_continue(repo: RepoId, engine: State<GixEngine>) -> Result<RebaseOutcome, String> {
    engine.rebase_continue(&repo).map_err(|e| e.to_string())
}

/// Skip the current commit of an in-progress rebase.
#[tauri::command]
fn rebase_skip(repo: RepoId, engine: State<GixEngine>) -> Result<RebaseOutcome, String> {
    engine.rebase_skip(&repo).map_err(|e| e.to_string())
}

/// The interactive-rebase range "from a commit to HEAD": the `onto` target plus
/// the commits to edit (oldest first), for seeding the rebase editor (PH3-004).
#[derive(Serialize)]
pub struct RebaseRange {
    pub onto: String,
    pub commits: Vec<CommitMeta>,
}

#[tauri::command]
fn rebase_range(
    repo: RepoId,
    from: String,
    engine: State<GixEngine>,
) -> Result<RebaseRange, String> {
    let (onto, commits) = engine
        .rebase_range(&repo, &Oid::from(from))
        .map_err(|e| e.to_string())?;
    Ok(RebaseRange {
        onto: onto.as_str().to_owned(),
        commits,
    })
}

/// A repository remembered in user settings, with an optional custom group.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct RecentRepo {
    pub path: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub family_id: Option<String>,
    #[serde(default)]
    pub family_name: Option<String>,
}

/// Persisted user settings (recent repos + their groups + custom commands +
/// an optional license key — not a secret per ADR-0007, so stored in plaintext
/// settings).
#[derive(Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub recent: Vec<RecentRepo>,
    #[serde(default)]
    pub custom_commands: Vec<lady_proto::CustomCommand>,
    #[serde(default)]
    pub license: Option<String>,
    /// AI configuration (provider, models, consent) — keys live in the keychain,
    /// never here (ADR-0008). Owned by the `ai_*` commands.
    #[serde(default)]
    pub ai: lady_ai::AiConfig,
    /// Repository family ids with AI explicitly DISABLED. AI is on by default;
    /// this is the opt-out list. Remote sends still require per-provider consent
    /// (ADR-0009).
    #[serde(default)]
    pub ai_disabled_repos: Vec<String>,
    /// Global defaults for the overridable settings (sign / ff / base / ai model).
    #[serde(default)]
    pub defaults: RepoSettings,
    /// Repository-family overrides keyed by common git directory path (same key
    /// as `ai_disabled_repos`). A field left `None` inherits from `defaults`,
    /// then the built-in fallback.
    #[serde(default)]
    pub repo_overrides: BTreeMap<String, RepoSettings>,
    /// Registered GitHub accounts (metadata only; PATs live in the keychain under
    /// `github-token:<id>`). Drives the per-repo HTTPS auth override and the
    /// confirm-once auto-suggest.
    #[serde(default)]
    pub github_accounts: Vec<GitHubAccount>,
    /// Repository-family ids where the user dismissed the account suggestion, so
    /// it is never offered again for that repo.
    #[serde(default)]
    pub auth_suggest_dismissed: Vec<String>,
}

/// Path to `settings.toml` in the platform config dir (via `directories`).
fn settings_file() -> Result<std::path::PathBuf, String> {
    let dirs = directories::ProjectDirs::from("dev", "Lady", "Lady")
        .ok_or_else(|| "could not resolve a config directory".to_string())?;
    Ok(dirs.config_dir().join("settings.toml"))
}

pub(crate) fn load_settings_inner() -> Settings {
    settings_file()
        .ok()
        .map(|p| load_settings_from(&p))
        .unwrap_or_default()
}

fn load_settings_from(path: &std::path::Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_settings_to(path: &std::path::Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, body).map_err(|e| e.to_string())
}

pub(crate) fn update_settings_inner(
    f: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> Result<(), String> {
    let path = settings_file()?;
    update_settings_at_path(&path, f)
}

fn update_settings_at_path(
    path: &std::path::Path,
    f: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> Result<(), String> {
    let _guard = SETTINGS_WRITE_LOCK
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    let mut settings = load_settings_from(path);
    f(&mut settings)?;
    write_settings_to(path, &settings)
}

#[tauri::command]
fn load_settings() -> Result<Settings, String> {
    Ok(load_settings_inner())
}

#[tauri::command]
fn save_settings(mut settings: Settings) -> Result<(), String> {
    update_settings_inner(|on_disk| {
        // The license is owned by the licensing commands and the AI
        // config/toggle by the ai_* commands; preserve whatever is on disk so a
        // recents/commands save can never clobber them (ADR-0007/0008/0009).
        settings.license = on_disk.license.clone();
        settings.ai = on_disk.ai.clone();
        settings.ai_disabled_repos = on_disk.ai_disabled_repos.clone();
        // Owned by the repo_settings / set_*_override / set_global_defaults commands.
        settings.defaults = on_disk.defaults.clone();
        settings.repo_overrides = on_disk.repo_overrides.clone();
        // Owned by the github account commands and the auth-suggest flow.
        settings.github_accounts = on_disk.github_accounts.clone();
        settings.auth_suggest_dismissed = on_disk.auth_suggest_dismissed.clone();
        *on_disk = settings;
        Ok(())
    })
}

/// Resolve a repo's family id — the repository-scoped key used by
/// `repo_overrides` and `ai_disabled_repos`, so linked worktrees share settings.
pub(crate) fn repo_settings_key(repo: &RepoId, engine: &GixEngine) -> Result<String, String> {
    Ok(engine
        .repository_family_id(repo)
        .map_err(|e| e.to_string())?
        .as_str()
        .to_string())
}

fn legacy_repo_settings_key(repo: &RepoId, engine: &GixEngine) -> Result<String, String> {
    Ok(engine
        .workdir_path(repo)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string())
}

fn repo_override_for(
    repo: &RepoId,
    engine: &GixEngine,
    settings: &Settings,
) -> Option<RepoSettings> {
    let key = repo_settings_key(repo, engine).ok()?;
    settings.repo_overrides.get(&key).cloned().or_else(|| {
        legacy_repo_settings_key(repo, engine)
            .ok()
            .and_then(|legacy| settings.repo_overrides.get(&legacy).cloned())
    })
}

/// One field's effective value: repo override, else global default, else `None`.
fn pick<T: Clone>(over: &Option<T>, global: &Option<T>) -> Option<T> {
    over.clone().or_else(|| global.clone())
}

/// `repo_settings` returns all three layers so the UI can prefill and show
/// inherited-vs-overridden in one round-trip.
#[derive(Serialize)]
pub struct ResolvedRepoSettings {
    /// Effective values (override ?? global), with built-in fallbacks left as the
    /// field default at the call site (the UI applies sign=false / ff=Auto).
    pub effective: RepoSettings,
    /// This repo's raw override (fields the user set for this repo only).
    pub r#override: RepoSettings,
    /// The global defaults.
    pub global: RepoSettings,
}

/// Read the effective + override + global settings for `repo`.
#[tauri::command]
fn repo_settings(repo: RepoId, engine: State<GixEngine>) -> Result<ResolvedRepoSettings, String> {
    let s = load_settings_inner();
    let over = repo_override_for(&repo, &engine, &s).unwrap_or_default();
    let global = s.defaults.clone();
    let effective = RepoSettings {
        sign: pick(&over.sign, &global.sign),
        ff: pick(&over.ff, &global.ff),
        base_branch: pick(&over.base_branch, &global.base_branch),
        ai_model: pick(&over.ai_model, &global.ai_model),
        // Auth is intentionally per-repo only — there is no sensible global
        // default account, so it never inherits.
        auth: over.auth.clone(),
    };
    Ok(ResolvedRepoSettings {
        effective,
        r#override: over,
        global,
    })
}

/// Replace this repo's override block (a field set to `None` reverts to inherit).
#[tauri::command]
fn set_repo_override(
    repo: RepoId,
    settings: RepoSettings,
    engine: State<GixEngine>,
) -> Result<(), String> {
    let key = repo_settings_key(&repo, &engine)?;
    let legacy_key = legacy_repo_settings_key(&repo, &engine).ok();
    update_settings_inner(|s| {
        if settings == RepoSettings::default() {
            s.repo_overrides.remove(&key);
        } else {
            if let Some(legacy_key) = &legacy_key {
                s.repo_overrides.remove(legacy_key);
            }
            s.repo_overrides.insert(key, settings);
        }
        Ok(())
    })
}

// ============================================================================
// Settings & Identity Commands
// ============================================================================

/// The global defaults block (no repo needed — drives Settings with no repo open).
#[tauri::command]
fn global_defaults() -> Result<RepoSettings, String> {
    Ok(load_settings_inner().defaults)
}

/// Replace the global defaults block.
#[tauri::command]
fn set_global_defaults(settings: RepoSettings) -> Result<(), String> {
    update_settings_inner(|s| {
        s.defaults = settings;
        Ok(())
    })
}

/// Read the repo's local git identity (`.git/config`).
#[tauri::command]
fn repo_identity_get(repo: RepoId, engine: State<GixEngine>) -> Result<GitIdentity, String> {
    engine.repo_identity_get(&repo).map_err(|e| e.to_string())
}

/// Write the repo's local git identity. Empty strings unset the keys.
#[tauri::command]
fn repo_identity_set(
    repo: RepoId,
    name: String,
    email: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .repo_identity_set(&repo, &name, &email)
        .map_err(|e| e.to_string())
}

// ── Multiple GitHub accounts — per-repo transport auth ───────────────────────────

/// Rewrite git's opaque HTTPS-auth failures into an actionable message. With
/// terminal prompts disabled (see `make_noninteractive`), an unauthenticated
/// remote op fails with "could not read Username … terminal prompts disabled" or
/// "Authentication failed" — point the user at the fix instead of git's raw text.
fn friendly_git_err(e: impl std::fmt::Display) -> String {
    let s = e.to_string();
    let low = s.to_lowercase();
    if low.contains("personal access token")
        && low.contains("workflow")
        && low.contains(".github/workflows")
    {
        return format!(
            "{s}\n\nGitHub rejected this push because the token used for this repo is missing the `workflow` scope. Re-add this GitHub account in Settings with a token that has `repo` and `workflow` scopes, or push with a credential helper/account that has workflow access."
        );
    }
    if low.contains("could not read username")
        || low.contains("could not read password")
        || low.contains("terminal prompts disabled")
        || low.contains("authentication failed")
    {
        return format!(
            "{s}\n\nAuthentication required for this remote. Connect a GitHub account in Settings, or configure a git credential helper / use an SSH remote."
        );
    }
    s
}

/// Wrap `s` in single quotes for a POSIX shell (git runs `!`-helpers and
/// `GIT_SSH_COMMAND` via the shell), escaping embedded single quotes.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build the per-invocation [`GitAuth`] for `repo` from its stored auth override.
/// Returns [`GitAuth::none`] (today's behavior — system git / `gh`) when no
/// override is set, so default repos are unaffected.
fn git_auth_for_repo(repo: &RepoId, engine: &GixEngine) -> GitAuth {
    let settings = load_settings_inner();
    let Some(over) = repo_override_for(repo, engine, &settings) else {
        return GitAuth::none();
    };
    match &over.auth {
        None => GitAuth::none(),
        Some(RepoAuth::SshKey(path)) => GitAuth {
            config: Vec::new(),
            env: vec![(
                "GIT_SSH_COMMAND".to_string(),
                format!("ssh -i {} -o IdentitiesOnly=yes", shell_single_quote(path)),
            )],
        },
        Some(RepoAuth::Account(id)) => {
            let login = settings
                .github_accounts
                .iter()
                .find(|a| &a.id == id)
                .map(|a| a.login.clone())
                .unwrap_or_else(|| id.clone());
            https_account_git_auth(id, &login)
        }
    }
}

/// Transient `git -c` config that routes HTTPS auth through Lady's own
/// credential helper for the given account — the token never appears in argv.
fn https_account_git_auth(account_id: &str, login: &str) -> GitAuth {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "lady".to_string());
    // A `!`-prefixed helper is run via the shell; quote the exe and account id.
    let helper = format!(
        "!{} git-credential --account {}",
        shell_single_quote(&exe),
        shell_single_quote(account_id),
    );
    GitAuth {
        config: vec![
            // Empty value resets any inherited helper list (e.g. gh) for this one
            // invocation; the next entry then installs only ours.
            ("credential.helper".to_string(), String::new()),
            ("credential.helper".to_string(), helper),
            // Hint the username so git's credential context targets this account.
            (
                "credential.https://github.com.username".to_string(),
                login.to_string(),
            ),
        ],
        env: Vec::new(),
    }
}

/// List the registered GitHub accounts (metadata only — no tokens).
#[tauri::command]
fn list_github_accounts() -> Result<Vec<GitHubAccount>, String> {
    Ok(load_settings_inner().github_accounts)
}

// ============================================================================
// Hosting & Authentication Commands (GitHub, GitLab, etc.)
// ============================================================================

/// Register (or update) a GitHub account: validate the PAT, learn the login,
/// store the token in the keychain under `github-token:<login>`, and persist the
/// metadata. Re-adding the same login refreshes its token + details.
#[tauri::command]
async fn add_github_account(
    name: String,
    email: String,
    known_owners: Vec<String>,
    token: String,
    hosting: State<'_, Hosting>,
) -> Result<GitHubAccount, String> {
    let login = lady_hosting::provider_by_kind(lady_hosting::ForgeKind::GitHub)
        .get_login(&token)
        .await
        .map_err(|e| e.to_string())?;
    let id = login.clone();
    hosting
        .store
        .set(&lady_hosting::github_account_token_key(&id), &token)
        .map_err(|e| e.to_string())?;
    let account = GitHubAccount {
        id: id.clone(),
        login,
        name,
        email,
        known_owners,
    };
    update_settings_inner(|s| {
        s.github_accounts.retain(|a| a.id != id);
        s.github_accounts.push(account.clone());
        Ok(())
    })?;
    Ok(account)
}

/// Remove a GitHub account: delete its keychain token, drop its metadata, and
/// revert any repos pinned to it back to the default credential helper.
#[tauri::command]
fn remove_github_account(id: String, hosting: State<Hosting>) -> Result<(), String> {
    hosting
        .store
        .delete(&lady_hosting::github_account_token_key(&id))
        .map_err(|e| e.to_string())?;
    update_settings_inner(|s| {
        s.github_accounts.retain(|a| a.id != id);
        for over in s.repo_overrides.values_mut() {
            if matches!(&over.auth, Some(RepoAuth::Account(a)) if a == &id) {
                over.auth = None;
            }
        }
        Ok(())
    })
}

/// A suggested account for a repo plus a short human reason.
#[derive(Serialize)]
pub struct AccountSuggestion {
    pub account: GitHubAccount,
    pub reason: String,
}

/// Suggest a GitHub account for `repo` by matching the remote owner against each
/// account's login / known owners. Returns `None` when the repo is already
/// pinned, the suggestion was dismissed, the remote isn't GitHub, or nothing
/// matches — so the UI only ever prompts once.
#[tauri::command]
fn suggest_repo_account(
    repo: RepoId,
    engine: State<GixEngine>,
    hosting: State<Hosting>,
) -> Result<Option<AccountSuggestion>, String> {
    let s = load_settings_inner();
    let key = repo_settings_key(&repo, &engine)?;
    let already_pinned = repo_override_for(&repo, &engine, &s)
        .and_then(|o| o.auth)
        .is_some();
    if already_pinned || s.auth_suggest_dismissed.iter().any(|d| d == &key) {
        return Ok(None);
    }
    let Some((provider, urls)) = provider_for_repo(&repo, &engine, &hosting)? else {
        return Ok(None);
    };
    if provider.kind() != lady_hosting::ForgeKind::GitHub {
        return Ok(None);
    }
    let Some(slug) = provider.detect_slug(&urls) else {
        return Ok(None);
    };
    let owner = slug.owner.to_lowercase();
    let matched = s.github_accounts.iter().find(|a| {
        a.login.to_lowercase() == owner || a.known_owners.iter().any(|o| o.to_lowercase() == owner)
    });
    Ok(matched.map(|a| AccountSuggestion {
        account: a.clone(),
        reason: format!(
            "Remote owner \u{201c}{}\u{201d} matches this account.",
            slug.owner
        ),
    }))
}

/// Pin `repo` to a GitHub account: set the HTTPS auth override and stamp the
/// account's identity into `.git/config` so commits are authored correctly.
#[tauri::command]
fn assign_repo_account(
    repo: RepoId,
    account_id: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    let key = repo_settings_key(&repo, &engine)?;
    let mut account = None;
    update_settings_inner(|s| {
        let found = s
            .github_accounts
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
            .ok_or_else(|| "Unknown GitHub account.".to_string())?;
        s.repo_overrides.entry(key).or_default().auth = Some(RepoAuth::Account(account_id));
        account = Some(found);
        Ok(())
    })?;
    let account = account.ok_or_else(|| "internal error: account not set".to_string())?;
    if !account.name.is_empty() || !account.email.is_empty() {
        engine
            .repo_identity_set(&repo, &account.name, &account.email)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Record that the user dismissed the account suggestion for `repo`, so it is
/// never offered again.
#[tauri::command]
fn dismiss_repo_account_suggestion(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    let key = repo_settings_key(&repo, &engine)?;
    update_settings_inner(|s| {
        if !s.auth_suggest_dismissed.iter().any(|d| d == &key) {
            s.auth_suggest_dismissed.push(key);
        }
        Ok(())
    })
}

/// Parse a credential-helper argv tail (`--account <id> <op>`) into the account
/// id and the git operation (`get` / `store` / `erase`).
fn parse_credential_args(args: &[String]) -> (Option<String>, Option<String>) {
    let mut account = None;
    let mut op = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--account" => account = it.next().cloned(),
            other => op = Some(other.to_string()),
        }
    }
    (account, op)
}

/// Build the git credential-helper response for account `id`: the account's PAT
/// (preferred) or the legacy single-account token, formatted as the credential
/// protocol. Returns `None` when no token is stored. Pure over its inputs so it
/// can be unit-tested with a mock [`TokenStore`].
fn credential_response(
    store: &dyn lady_hosting::TokenStore,
    accounts: &[GitHubAccount],
    id: &str,
) -> Option<String> {
    let token = store
        .get(&lady_hosting::github_account_token_key(id))
        .ok()
        .flatten()
        .or_else(|| {
            store
                .get(lady_hosting::ForgeKind::GitHub.token_key())
                .ok()
                .flatten()
        })?;
    let login = accounts
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.login.clone())
        .unwrap_or_else(|| id.to_string());
    // git reads username/password from stdout; a trailing blank line ends it.
    Some(format!("username={login}\npassword={token}\n\n"))
}

/// Credential-helper mode: when git invokes `<exe> git-credential --account <id>
/// <op>`, serve that account's PAT from the keychain on `get` (and no-op on
/// `store`/`erase` — Lady owns the token, git must not overwrite it). Reads from
/// the same keychain service as the app so the running UI and this subprocess
/// agree. Prints the git credential protocol to stdout and returns.
fn run_credential_helper(args: &[String]) {
    let (account, op) = parse_credential_args(args);
    if op.as_deref() != Some("get") {
        return;
    }
    let Some(id) = account else { return };
    let store = lady_hosting::KeyringStore::new(KEYCHAIN_SERVICE);
    let accounts = load_settings_inner().github_accounts;
    if let Some(out) = credential_response(&store, &accounts, &id) {
        print!("{out}");
    }
}

// ── Hosting (GitHub) — PH3-011 / PH3-012 ────────────────────────────────────────

/// When the target remote is HTTPS and a hosting token is stored for that
/// forge, return it so fetch/pull/push can authenticate as the connected user
/// instead of stale system credential-helper entries.
fn http_bearer_for_remote(
    repo: &RepoId,
    remote: Option<&str>,
    engine: &GixEngine,
    hosting: &Hosting,
) -> Option<String> {
    let remote_name = remote.unwrap_or("origin");
    let url = engine.remote_url(repo, remote_name).ok()?;
    if !url.starts_with("https://") {
        return None;
    }
    let provider = lady_hosting::provider_for(&url, &hosting.self_hosted)?;
    hosting
        .store
        .get(provider.token_key())
        .ok()
        .flatten()
        .filter(|t| !t.is_empty())
}

fn http_bearer_git_auth(token: String) -> GitAuth {
    GitAuth {
        config: vec![("credential.helper".to_string(), String::new())],
        env: vec![(
            "GIT_HTTP_EXTRAHEADER".to_string(),
            format!("Authorization: Bearer {token}"),
        )],
    }
}

/// Managed hosting state: the OS-keychain token store + any self-hosted forge
/// configs for resolution.
pub struct Hosting {
    store: Box<dyn lady_hosting::TokenStore>,
    self_hosted: Vec<lady_hosting::ForgeConfig>,
}

/// Connection status for the active repo's forge (no token is ever returned).
#[derive(Serialize)]
pub struct HostingInfo {
    /// The detected forge (`None` when no supported remote).
    pub kind: Option<lady_hosting::ForgeKind>,
    /// Whether a valid token is stored for that forge.
    pub connected: bool,
    /// The authenticated login/handle, when known.
    pub login: Option<String>,
    /// The detected repo slug on that forge.
    pub slug: Option<lady_hosting::RepoSlug>,
}

/// A resolved provider plus the repo's remote URLs.
type ResolvedProvider = (Box<dyn lady_hosting::HostingProvider>, Vec<String>);

/// Resolve the hosting provider for `repo` from its remotes (forge-agnostic).
fn provider_for_repo(
    repo: &RepoId,
    engine: &GixEngine,
    hosting: &Hosting,
) -> Result<Option<ResolvedProvider>, String> {
    let urls = engine.list_remote_urls(repo).map_err(|e| e.to_string())?;
    let provider = urls
        .iter()
        .find_map(|u| lady_hosting::provider_for(u, &hosting.self_hosted));
    Ok(provider.map(|p| (p, urls)))
}

/// The API token for `repo`'s forge: the repo's pinned GitHub account token when
/// one is assigned, otherwise the forge's legacy single-account token. Keeps the
/// hosting API consistent with the per-repo transport account.
fn repo_forge_token(
    repo: &RepoId,
    engine: &GixEngine,
    provider: &dyn lady_hosting::HostingProvider,
    hosting: &Hosting,
) -> Result<Option<String>, String> {
    if provider.kind() == lady_hosting::ForgeKind::GitHub {
        let s = load_settings_inner();
        if let Some(RepoAuth::Account(id)) =
            repo_override_for(repo, engine, &s).and_then(|o| o.auth)
        {
            if let Some(tok) = hosting
                .store
                .get(&lady_hosting::github_account_token_key(&id))
                .map_err(|e| e.to_string())?
            {
                return Ok(Some(tok));
            }
        }
    }
    hosting
        .store
        .get(provider.token_key())
        .map_err(|e| e.to_string())
}

/// Connection status for the active repo's forge (PH4-001/002/003/004).
#[tauri::command]
async fn hosting_status(
    repo: RepoId,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<HostingInfo, String> {
    let Some((provider, urls)) = provider_for_repo(&repo, &engine, &hosting)? else {
        return Ok(HostingInfo {
            kind: None,
            connected: false,
            login: None,
            slug: None,
        });
    };
    let kind = provider.kind();
    let slug = provider.detect_slug(&urls);
    let Some(token) = repo_forge_token(&repo, &engine, provider.as_ref(), &hosting)? else {
        return Ok(HostingInfo {
            kind: Some(kind),
            connected: false,
            login: None,
            slug,
        });
    };
    // Best-effort: confirm validity + fetch the login.
    let (connected, login) = match provider.get_login(&token).await {
        Ok(login) => (true, Some(login)),
        Err(lady_hosting::Error::Unauthorized) => (false, None),
        Err(_) => (true, None), // network hiccup; token exists
    };
    Ok(HostingInfo {
        kind: Some(kind),
        connected,
        login,
        slug,
    })
}

/// Connect to the active repo's forge with a token: validate it, then store it
/// under that forge's keychain key (ADR-0006). Never logs the token.
#[tauri::command]
async fn hosting_connect(
    repo: RepoId,
    token: String,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<HostingInfo, String> {
    let (provider, urls) = provider_for_repo(&repo, &engine, &hosting)?
        .ok_or_else(|| "No supported forge remote found on this repository.".to_string())?;
    let login = provider
        .get_login(&token)
        .await
        .map_err(|e| e.to_string())?;
    hosting
        .store
        .set(provider.token_key(), &token)
        .map_err(|e| e.to_string())?;
    Ok(HostingInfo {
        kind: Some(provider.kind()),
        connected: true,
        login: Some(login),
        slug: provider.detect_slug(&urls),
    })
}

/// List the authenticated user's GitHub notifications (PH4-006). Requires a
/// stored GitHub token.
#[tauri::command]
async fn github_notifications(
    hosting: State<'_, Hosting>,
) -> Result<Vec<lady_hosting::Notification>, String> {
    let token = hosting
        .store
        .get(lady_hosting::ForgeKind::GitHub.token_key())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Not connected to GitHub — connect in Settings first.".to_string())?;
    lady_hosting::GitHubClient::new()
        .list_notifications(&token)
        .await
        .map_err(|e| e.to_string())
}

/// Mark a GitHub notification thread read (PH4-006).
#[tauri::command]
async fn github_mark_read(id: String, hosting: State<'_, Hosting>) -> Result<(), String> {
    let token = hosting
        .store
        .get(lady_hosting::ForgeKind::GitHub.token_key())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Not connected to GitHub.".to_string())?;
    lady_hosting::GitHubClient::new()
        .mark_read(&token, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Create a remote repository on `forge` and return its URLs (PH4-005).
/// Optionally wires the clone URL as `origin` on `add_origin_to` repo.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn create_remote_repo(
    forge: lady_hosting::ForgeKind,
    name: String,
    private: bool,
    description: String,
    owner: Option<String>,
    project: Option<String>,
    add_origin_to: Option<RepoId>,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<lady_hosting::RepoInfo, String> {
    let provider = lady_hosting::provider_by_kind(forge);
    let token = hosting
        .store
        .get(provider.token_key())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Not connected to {} — connect in Settings first.",
                forge.label()
            )
        })?;
    let spec = lady_hosting::NewRepo {
        name,
        private,
        description,
        owner,
        project,
    };
    let info = provider
        .create_repo(&token, &spec)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(repo) = add_origin_to {
        // Best-effort: wire the new remote as origin.
        engine
            .add_remote(&repo, "origin", &info.clone_url)
            .map_err(|e| e.to_string())?;
    }
    Ok(info)
}

/// Forget the stored token for the active repo's forge.
#[tauri::command]
async fn hosting_sign_out(
    repo: RepoId,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<(), String> {
    if let Some((provider, _)) = provider_for_repo(&repo, &engine, &hosting)? {
        hosting
            .store
            .delete(provider.token_key())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Open a pull / merge request for the active repo's forge remote, resolved by
/// the remote URL (PH3-012 / PH4-001). Returns the PR's web URL. Errors clearly
/// on no-auth / no-supported-remote / API failures (e.g. a PR already exists).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn github_create_pr(
    repo: RepoId,
    head: String,
    base: String,
    title: String,
    body: String,
    draft: bool,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<String, String> {
    let urls = engine.list_remote_urls(&repo).map_err(|e| e.to_string())?;
    // Resolve the forge from the first supported remote.
    let provider = urls
        .iter()
        .find_map(|u| lady_hosting::provider_for(u, &hosting.self_hosted))
        .ok_or_else(|| "No supported forge remote found on this repository.".to_string())?;
    let slug = provider.detect_slug(&urls).ok_or_else(|| {
        format!(
            "Could not parse a {} repository from the remotes.",
            provider.kind().label()
        )
    })?;
    let token =
        repo_forge_token(&repo, &engine, provider.as_ref(), &hosting)?.ok_or_else(|| {
            format!(
                "Not connected to {} — connect in Settings first.",
                provider.kind().label()
            )
        })?;
    let pr = lady_hosting::NewPullRequest {
        head,
        base,
        title,
        body,
        draft,
    };
    provider
        .create_pull_request(&token, &slug, &pr)
        .await
        .map_err(|e| e.to_string())
}

/// Resolve the forge provider, slug, and stored token for `repo` (shared by the
/// PR/issue list commands). Errors with a clear message when not connected.
fn resolve_forge(
    repo: &RepoId,
    engine: &GixEngine,
    hosting: &Hosting,
) -> Result<
    (
        Box<dyn lady_hosting::HostingProvider>,
        lady_hosting::RepoSlug,
        String,
    ),
    String,
> {
    let urls = engine.list_remote_urls(repo).map_err(|e| e.to_string())?;
    let provider = urls
        .iter()
        .find_map(|u| lady_hosting::provider_for(u, &hosting.self_hosted))
        .ok_or_else(|| "No supported forge remote found on this repository.".to_string())?;
    let slug = provider.detect_slug(&urls).ok_or_else(|| {
        format!(
            "Could not parse a {} repository from the remotes.",
            provider.kind().label()
        )
    })?;
    let token = repo_forge_token(repo, engine, provider.as_ref(), hosting)?.ok_or_else(|| {
        format!(
            "Not connected to {} — connect in Settings first.",
            provider.kind().label()
        )
    })?;
    Ok((provider, slug, token))
}

/// Build a browser URL for `target` (commit/branch/tag) on the active repo's
/// forge, or `None` when no supported remote/slug is detected. No token needed —
/// this is pure URL assembly so "Copy link" works even when not signed in.
#[tauri::command]
fn remote_web_url(
    repo: RepoId,
    target: lady_hosting::WebTarget,
    engine: State<GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<Option<String>, String> {
    let Some((provider, urls)) = provider_for_repo(&repo, &engine, &hosting)? else {
        return Ok(None);
    };
    let Some(slug) = provider.detect_slug(&urls) else {
        return Ok(None);
    };
    // Web base = the matching remote's host, so self-hosted installs resolve to
    // their own host rather than the forge's public one.
    let web_base = urls
        .iter()
        .find(|u| {
            lady_hosting::provider_for(u, &hosting.self_hosted).map(|p| p.kind())
                == Some(provider.kind())
        })
        .and_then(|u| lady_hosting::web_base(u));
    Ok(web_base.map(|base| provider.web_url(&base, &slug, &target)))
}

/// List open pull/merge requests for the active repo's forge.
#[tauri::command]
async fn list_pull_requests(
    repo: RepoId,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<Vec<lady_hosting::ForgeItem>, String> {
    let (provider, slug, token) = resolve_forge(&repo, &engine, &hosting)?;
    provider
        .list_pull_requests(&token, &slug)
        .await
        .map_err(|e| e.to_string())
}

/// List open issues for the active repo's forge.
#[tauri::command]
async fn list_issues(
    repo: RepoId,
    engine: State<'_, GixEngine>,
    hosting: State<'_, Hosting>,
) -> Result<Vec<lady_hosting::ForgeItem>, String> {
    let (provider, slug, token) = resolve_forge(&repo, &engine, &hosting)?;
    provider
        .list_issues(&token, &slug)
        .await
        .map_err(|e| e.to_string())
}

/// Open `url` in the user's default browser (used to view a freshly opened PR).
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    use std::process::Command;
    // Only http(s) URLs are opened, never arbitrary commands.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("refusing to open a non-http URL".to_string());
    }
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(&url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", &url]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(&url);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Open a repo-relative `path` with the OS default application.
#[tauri::command]
fn open_path(repo: RepoId, path: String, engine: State<GixEngine>) -> Result<(), String> {
    use std::process::Command;
    let abs = engine
        .resolve_path(&repo, &path)
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(&abs);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c.arg(&abs);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(&abs);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Reveal a repo-relative `path` in the OS file manager (selecting it).
#[tauri::command]
fn reveal_path(repo: RepoId, path: String, engine: State<GixEngine>) -> Result<(), String> {
    use std::process::Command;
    let abs = engine
        .resolve_path(&repo, &path)
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg("-R").arg(&abs);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("explorer");
        // explorer treats a non-zero exit as success oddly; /select, highlights it.
        c.arg(format!("/select,{}", abs.display()));
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        // No portable "select" on Linux; open the containing directory instead.
        let dir = abs.parent().unwrap_or(&abs).to_path_buf();
        let mut c = Command::new("xdg-open");
        c.arg(dir);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Rename branch `old` to `new` (`git branch -m`).
#[tauri::command]
fn rename_branch(
    repo: RepoId,
    old: String,
    new: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .rename_branch(&repo, &old, &new)
        .map_err(|e| e.to_string())
}

/// The short upstream of `branch` (e.g. `origin/main`), or `None` when unset.
#[tauri::command]
fn branch_upstream(
    repo: RepoId,
    branch: String,
    engine: State<GixEngine>,
) -> Result<Option<String>, String> {
    engine
        .branch_upstream(&repo, &branch)
        .map_err(|e| e.to_string())
}

/// Set (`Some`) or unset (`None`) the upstream tracking ref of `branch`.
#[tauri::command]
fn set_branch_upstream(
    repo: RepoId,
    branch: String,
    upstream: Option<String>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .set_branch_upstream(&repo, &branch, upstream.as_deref())
        .map_err(|e| e.to_string())
}

/// Fast-forward local `branch` to `upstream` without checking it out.
#[tauri::command]
fn fast_forward_branch(
    repo: RepoId,
    branch: String,
    upstream: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .fast_forward_branch(&repo, &branch, &upstream)
        .map_err(|e| e.to_string())
}

/// Discard working-tree + index changes to tracked `paths` (`git checkout HEAD`).
#[tauri::command]
fn discard_files(repo: RepoId, paths: Vec<String>, engine: State<GixEngine>) -> Result<(), String> {
    engine
        .discard_files(&repo, &paths)
        .map_err(|e| e.to_string())
}

/// Stash only `paths` (`git stash push [-u] [-m] -- …`).
#[tauri::command]
fn stash_paths(
    repo: RepoId,
    message: Option<String>,
    include_untracked: bool,
    paths: Vec<String>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .stash_paths(&repo, message.as_deref(), include_untracked, &paths)
        .map_err(|e| e.to_string())
}

/// Write the uncommitted diff of `paths` to `dest` (`git diff HEAD -- …`).
#[tauri::command]
fn export_patch(
    repo: RepoId,
    paths: Vec<String>,
    dest: String,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .export_patch(&repo, &paths, &dest)
        .map_err(|e| e.to_string())
}

/// Append `patterns` (one per line) to the repo-root `.gitignore`.
#[tauri::command]
fn add_to_gitignore(
    repo: RepoId,
    patterns: Vec<String>,
    engine: State<GixEngine>,
) -> Result<(), String> {
    engine
        .add_to_gitignore(&repo, &patterns)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Licensing Commands
// ============================================================================

// ── Licensing gate — PH3-013 (ADR-0007: client-side speed bump, NOT DRM) ────────

/// Current Unix time in seconds.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Path to the first-run timestamp file in the platform config dir.
fn trial_file() -> Result<std::path::PathBuf, String> {
    let dirs = directories::ProjectDirs::from("dev", "Lady", "Lady")
        .ok_or_else(|| "could not resolve a config directory".to_string())?;
    Ok(dirs.config_dir().join("trial"))
}

/// Read the recorded first-run timestamp, creating it (now) on first run.
fn trial_started() -> Result<i64, String> {
    let path = trial_file()?;
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(ts) = s.trim().parse::<i64>() {
            return Ok(ts);
        }
    }
    let now = now_secs();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, now.to_string()).map_err(|e| e.to_string())?;
    Ok(now)
}

/// The app's current licensing status (Trial / Expired / Licensed). Records the
/// first-run timestamp on first call.
#[tauri::command]
fn license_status() -> Result<lady_license::LicenseStatus, String> {
    let started = trial_started()?;
    let license = load_settings_inner().license;
    Ok(lady_license::evaluate_embedded(
        license.as_deref(),
        started,
        now_secs(),
    ))
}

/// Activate a license key: verify it offline against the embedded key; on
/// success persist it and return the new status. Rejects tampered / expired /
/// wrong-product keys with git-free, clear errors.
#[tauri::command]
fn license_activate(key: String) -> Result<lady_license::LicenseStatus, String> {
    let key = key.trim().to_string();
    // Verify before persisting; surface the precise rejection reason.
    lady_license::verify_embedded(&key, now_secs()).map_err(|e| e.to_string())?;
    update_settings_inner(|settings| {
        settings.license = Some(key);
        Ok(())
    })?;
    license_status()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Credential-helper mode: git invokes us as `<exe> git-credential --account
    // <id> <op>`. Serve the account's PAT and exit before any UI is created.
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("git-credential") {
        run_credential_helper(&argv[2..]);
        return;
    }

    let builder = tauri::Builder::default();
    // The auto-updater is desktop-only; mobile builds omit it entirely.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    builder
        .plugin(tauri_plugin_dialog::init())
        .manage(GixEngine::new())
        .manage(Hosting {
            store: Box::new(lady_hosting::KeyringStore::new(KEYCHAIN_SERVICE)),
            self_hosted: Vec::new(),
        })
        .manage(ai::AiState::new())
        .manage(watcher::RepoWatchers::new())
        .invoke_handler(tauri::generate_handler![
            app_info,
            open_repo,
            list_refs,
            walk_log,
            walk_log_graph,
            diff,
            diff_spec,
            blame,
            file_history,
            repo_dirty,
            list_files,
            status,
            stage_paths,
            unstage_paths,
            stage_hunks,
            unstage_hunks,
            stage_lines,
            discard_hunks,
            discard_lines,
            discard_untracked,
            commit,
            recent_messages,
            create_branch,
            delete_branch,
            checkout,
            create_tag,
            delete_tag,
            move_tag,
            reset,
            fetch,
            fetch_background,
            watch_repo,
            unwatch_repo,
            pull,
            push,
            delete_remote_ref,
            remote_web_url,
            ahead_behind,
            branches_ahead_behind,
            stash_save,
            stash_list,
            stash_apply,
            stash_pop,
            stash_drop,
            merge,
            merge_abort,
            cherry_pick,
            revert,
            sequencer_abort,
            rebase,
            rebase_abort,
            list_conflicts,
            conflict_sides,
            parse_conflict,
            take_ours,
            take_theirs,
            write_resolution,
            mark_resolved,
            conflict_state,
            conflict_abort,
            conflict_continue,
            rebase_interactive,
            rebase_continue,
            rebase_skip,
            rebase_range,
            signature_statuses,
            list_worktrees,
            repository_family,
            repository_family_identity,
            add_worktree,
            remove_worktree,
            prune_worktrees,
            reflog,
            lfs_status,
            lfs_track,
            flow_config,
            flow_init,
            flow_start,
            flow_finish,
            list_submodules,
            add_submodule,
            init_submodules,
            update_submodules,
            sync_submodules,
            deinit_submodule,
            bisect_start,
            bisect_mark,
            bisect_reset,
            bisect_state,
            parse_placeholders,
            run_custom_command,
            launch_difftool,
            launch_mergetool,
            hosting_status,
            hosting_connect,
            hosting_sign_out,
            create_remote_repo,
            github_notifications,
            github_mark_read,
            github_create_pr,
            list_pull_requests,
            list_issues,
            open_url,
            open_path,
            reveal_path,
            rename_branch,
            branch_upstream,
            set_branch_upstream,
            fast_forward_branch,
            discard_files,
            stash_paths,
            export_patch,
            add_to_gitignore,
            license_status,
            license_activate,
            clone_repo,
            load_settings,
            save_settings,
            repo_settings,
            set_repo_override,
            global_defaults,
            set_global_defaults,
            repo_identity_get,
            repo_identity_set,
            list_github_accounts,
            add_github_account,
            remove_github_account,
            suggest_repo_account,
            assign_repo_account,
            dismiss_repo_account_suggestion,
            ai::ai_get_config,
            ai::ai_set_config,
            ai::ai_set_key,
            ai::ai_delete_key,
            ai::ai_has_key,
            ai::ai_grant_consent,
            ai::ai_revoke_consent,
            ai::ai_set_repo_enabled,
            ai::ai_repo_enabled,
            ai::ai_list_models,
            ai::ai_cancel,
            ai::ai_commit_message,
            ai::ai_compose_commits,
            ai::ai_apply_commit_plan,
            ai::ai_recompose_plan,
            ai::ai_recompose_apply,
            ai::ai_explain,
            ai::ai_resolve_conflict,
            ai::ai_pr_title,
            ai::ai_pr_description,
            ai::ai_changelog,
            ai::ai_stash_note,
            // ============================================================================
            // Updater Commands (Desktop Only)
            // ============================================================================
            #[cfg(desktop)]
            updater::check_for_updates,
            #[cfg(desktop)]
            updater::install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use lady_hosting::TokenStore;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// In-memory [`TokenStore`] so credential-helper logic can be tested without
    /// touching the OS keychain (unavailable on headless CI).
    #[derive(Default)]
    struct MockStore(Mutex<HashMap<String, String>>);
    impl lady_hosting::TokenStore for MockStore {
        fn get(&self, key: &str) -> lady_hosting::Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> lady_hosting::Result<()> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        fn delete(&self, key: &str) -> lady_hosting::Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn account(id: &str, login: &str) -> GitHubAccount {
        GitHubAccount {
            id: id.to_string(),
            login: login.to_string(),
            name: String::new(),
            email: String::new(),
            known_owners: Vec::new(),
        }
    }

    #[test]
    fn parse_credential_args_extracts_account_and_op() {
        let args = ["--account".into(), "octocat".into(), "get".into()];
        let (acct, op) = parse_credential_args(&args);
        assert_eq!(acct.as_deref(), Some("octocat"));
        assert_eq!(op.as_deref(), Some("get"));
    }

    #[test]
    fn credential_response_returns_account_token() {
        let store = MockStore::default();
        store
            .set(&lady_hosting::github_account_token_key("work"), "tok-work")
            .unwrap();
        let accounts = vec![account("work", "work-login")];
        let out = credential_response(&store, &accounts, "work").expect("token present");
        assert_eq!(out, "username=work-login\npassword=tok-work\n\n");
    }

    #[test]
    fn credential_response_falls_back_to_legacy_token() {
        let store = MockStore::default();
        store
            .set(lady_hosting::ForgeKind::GitHub.token_key(), "legacy-tok")
            .unwrap();
        // No per-account token, no metadata → falls back to legacy + id as login.
        let out = credential_response(&store, &[], "octocat").expect("legacy token");
        assert_eq!(out, "username=octocat\npassword=legacy-tok\n\n");
    }

    #[test]
    fn credential_response_none_when_unknown() {
        let store = MockStore::default();
        assert!(credential_response(&store, &[], "nobody").is_none());
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("git must be installed")
            .success();
        assert!(ok, "git {args:?} failed");
    }

    fn fixture() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Test"]);
        git(p, &["config", "user.email", "t@t.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);
        for i in 1..=5 {
            std::fs::write(p.join(format!("f{i}.txt")), format!("{i}")).expect("write");
            git(p, &["add", "."]);
            git(p, &["commit", "-q", "-m", &format!("commit {i}")]);
        }
        dir
    }

    #[test]
    fn settings_round_trip_preserves_defaults_and_overrides() {
        let mut s = Settings {
            defaults: RepoSettings {
                sign: Some(true),
                ff: Some(FfMode::Only),
                base_branch: None,
                ai_model: Some("claude-opus-4-8".to_string()),
                auth: None,
            },
            ..Default::default()
        };
        s.repo_overrides.insert(
            "/repo/a".to_string(),
            RepoSettings {
                ff: Some(FfMode::Never),
                base_branch: Some("develop".to_string()),
                ..Default::default()
            },
        );

        let toml = toml::to_string_pretty(&s).expect("serialize settings");
        let back: Settings = toml::from_str(&toml).expect("deserialize settings");

        assert_eq!(back.defaults.sign, Some(true));
        assert_eq!(back.defaults.ff, Some(FfMode::Only));
        assert_eq!(back.defaults.ai_model.as_deref(), Some("claude-opus-4-8"));
        let a = back
            .repo_overrides
            .get("/repo/a")
            .expect("override for /repo/a");
        assert_eq!(a.ff, Some(FfMode::Never));
        assert_eq!(a.base_branch.as_deref(), Some("develop"));
        assert_eq!(a.sign, None, "unset fields stay None (inherit)");
    }

    #[test]
    fn settings_round_trip_preserves_ai_disabled_repos() {
        let mut s = Settings::default();
        s.ai_disabled_repos.push("/repo/disabled".to_string());
        s.ai.active = Some(lady_ai::ProviderKind::OpenAi);

        let toml = toml::to_string_pretty(&s).expect("serialize settings");
        let mut back: Settings = toml::from_str(&toml).expect("deserialize settings");

        // Simulate ai_set_config: replace ai config but preserve the opt-out list.
        let consented = back.ai.consented.clone();
        let ai_disabled_repos = back.ai_disabled_repos.clone();
        let repo_overrides = back.repo_overrides.clone();
        back.ai = lady_ai::AiConfig::default();
        back.ai.consented = consented;
        back.ai_disabled_repos = ai_disabled_repos;
        back.repo_overrides = repo_overrides;

        let toml2 = toml::to_string_pretty(&back).expect("serialize settings");
        let back2: Settings = toml::from_str(&toml2).expect("deserialize settings");

        assert_eq!(back2.ai_disabled_repos, vec!["/repo/disabled"]);
        assert_eq!(back2.ai.active, None, "ai config was replaced");
    }

    #[test]
    fn settings_tolerates_missing_new_fields() {
        // An old settings file with no defaults/overrides still loads.
        let s: Settings = toml::from_str("recent = []\n").expect("deserialize legacy settings");
        assert_eq!(s.defaults, RepoSettings::default());
        assert!(s.repo_overrides.is_empty());
    }

    #[test]
    fn settings_update_helper_preserves_unrelated_sections() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        update_settings_at_path(&path, |settings| {
            settings.github_accounts.push(GitHubAccount {
                id: "acct".to_string(),
                login: "acct".to_string(),
                name: "A User".to_string(),
                email: "a@example.test".to_string(),
                known_owners: vec!["org".to_string()],
            });
            settings.defaults.sign = Some(true);
            Ok(())
        })
        .expect("first update");

        update_settings_at_path(&path, |settings| {
            settings.recent.push(RecentRepo {
                path: "/repo/one".to_string(),
                group: Some("work".to_string()),
                family_id: Some("family".to_string()),
                family_name: Some("Repo".to_string()),
            });
            settings.repo_overrides.insert(
                "family".to_string(),
                RepoSettings {
                    ff: Some(FfMode::Only),
                    ..Default::default()
                },
            );
            Ok(())
        })
        .expect("second update");

        let back = load_settings_from(&path);
        assert_eq!(back.github_accounts.len(), 1);
        assert_eq!(back.github_accounts[0].login, "acct");
        assert_eq!(back.defaults.sign, Some(true));
        assert_eq!(back.recent.len(), 1);
        assert_eq!(back.recent[0].path, "/repo/one");
        assert_eq!(
            back.repo_overrides.get("family").and_then(|s| s.ff),
            Some(FfMode::Only)
        );
    }

    fn rev(dir: &Path, r: &str) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", r])
            .output()
            .expect("git rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Build a repo with a base commit then a messy span (one file per commit).
    fn messy_repo() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "T"]);
        git(p, &["config", "user.email", "t@t.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);
        std::fs::write(p.join("a.txt"), "base\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "base"]);
        for f in ["x", "y", "z"] {
            std::fs::write(p.join(format!("{f}.txt")), format!("{f}\n")).expect("write");
            git(p, &["add", "."]);
            git(p, &["commit", "-q", "-m", &format!("wip {f}")]);
        }
        dir
    }

    #[test]
    fn recompose_regroups_span_and_preserves_tree() {
        use lady_ai::prompts::{CommitPlan, PlannedCommit};
        let dir = messy_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let base = rev(p, "HEAD~3"); // the "base" commit
        let from = rev(p, "HEAD~2"); // first messy commit (wip x)

        // Regroup x+y into one commit and z into another (3 commits → 2).
        let plan = CommitPlan {
            commits: vec![
                PlannedCommit {
                    message: "feat: x and y".to_string(),
                    hunk_ids: vec!["x.txt:0".to_string(), "y.txt:0".to_string()],
                },
                PlannedCommit {
                    message: "feat: z".to_string(),
                    hunk_ids: vec!["z.txt:0".to_string()],
                },
            ],
        };

        let made = ai::recompose_apply_inner(&id, &from, &plan, &engine).expect("recompose");
        assert_eq!(made, 2);

        // The span is now 2 commits on top of base, with the same files/content.
        let count = std::process::Command::new("git")
            .current_dir(p)
            .args(["rev-list", "--count", &format!("{base}..HEAD")])
            .output()
            .expect("rev-list");
        assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "2");
        for (f, c) in [
            ("a.txt", "base\n"),
            ("x.txt", "x\n"),
            ("y.txt", "y\n"),
            ("z.txt", "z\n"),
        ] {
            assert_eq!(std::fs::read_to_string(p.join(f)).expect("read"), c);
        }
        // Clean tree afterwards.
        let wt = engine.status(&id).expect("status");
        assert!(wt.staged.is_empty() && wt.unstaged.is_empty() && wt.untracked.is_empty());
    }

    #[test]
    fn recompose_failure_rolls_back_to_original_head() {
        use lady_ai::prompts::{CommitPlan, PlannedCommit};
        let dir = messy_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");
        let orig = rev(p, "HEAD");
        let from = rev(p, "HEAD~2");

        // A plan referencing a non-existent path makes apply fail mid-flight.
        let bad = CommitPlan {
            commits: vec![PlannedCommit {
                message: "broken".to_string(),
                hunk_ids: vec!["nope.txt:0".to_string()],
            }],
        };
        let err = ai::recompose_apply_inner(&id, &from, &bad, &engine).expect_err("should fail");
        assert!(err.contains("rolled back"), "got: {err}");

        // History is intact: HEAD back at the original tip, tree clean.
        assert_eq!(rev(p, "HEAD"), orig);
        let wt = engine.status(&id).expect("status");
        assert!(wt.staged.is_empty() && wt.unstaged.is_empty() && wt.untracked.is_empty());
    }

    #[test]
    fn command_open_and_list_refs() {
        let dir = fixture();
        let engine = GixEngine::new();
        let id = engine
            .open(dir.path())
            .map_err(|e| e.to_string())
            .expect("open_repo command logic");
        let refs = engine
            .list_refs(&id)
            .map_err(|e| e.to_string())
            .expect("list_refs command logic");
        assert!(
            refs.iter().any(|r| r.kind == lady_proto::RefKind::Branch),
            "should include a branch ref"
        );
        assert!(
            refs.iter().any(|r| r.kind == lady_proto::RefKind::Head),
            "should include HEAD"
        );
    }

    #[test]
    fn command_walk_log_paged() {
        let dir = fixture();
        let engine = GixEngine::new();
        let id = engine
            .open(dir.path())
            .map_err(|e| e.to_string())
            .expect("open_repo");
        // All 5 commits with no limit cap.
        let all = engine
            .walk_log(
                &id,
                GraphQuery {
                    start: None,
                    limit: 0,
                },
            )
            .map_err(|e| e.to_string())
            .expect("walk_log all");
        assert_eq!(all.len(), 5);

        // Paged: first 3.
        let page1 = engine
            .walk_log(
                &id,
                GraphQuery {
                    start: None,
                    limit: 3,
                },
            )
            .map_err(|e| e.to_string())
            .expect("walk_log page1");
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0].summary, "commit 5");

        // Next page: start from page1's last commit (inclusive) with limit+1, skip first.
        let cursor = page1.last().unwrap().oid.clone();
        let page2_raw = engine
            .walk_log(
                &id,
                GraphQuery {
                    start: Some(cursor),
                    limit: 4,
                },
            )
            .map_err(|e| e.to_string())
            .expect("walk_log page2");
        // Skip the overlap (cursor commit itself) → 2 remaining commits.
        let page2: Vec<_> = page2_raw.into_iter().skip(1).collect();
        assert_eq!(page2.len(), 2, "remaining commits after page1");
        assert_eq!(page2.last().unwrap().summary, "commit 1");
    }
}
