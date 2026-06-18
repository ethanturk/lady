use lady_git::{CommitOpts, DiffSpec, GitEngine, GixEngine, GraphQuery, MergeOpts};
use lady_graph::layout_continuation;
use lady_proto::{
    ApplyOutcome, Blame, CommitMeta, FfMode, FileDiff, MergeOutcome, Oid, RebaseOutcome, RefInfo,
    RepoId, WorkingTree,
};
use serde::{Deserialize, Serialize};
use tauri::State;

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
fn walk_log(
    repo: RepoId,
    query: WalkLogQuery,
    engine: State<GixEngine>,
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
fn walk_log_graph(
    repo: RepoId,
    query: WalkLogQuery,
    layout_state: Option<Vec<Option<String>>>,
    engine: State<GixEngine>,
) -> Result<WalkLogGraphResult, String> {
    let gq = GraphQuery {
        start: query.start.map(Oid::from),
        limit: query.limit,
    };
    let commits = engine.walk_log(&repo, gq).map_err(|e| e.to_string())?;

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
            refs: r.refs,
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

#[tauri::command]
fn diff(repo: RepoId, commit: String, engine: State<GixEngine>) -> Result<Vec<FileDiff>, String> {
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
fn diff_spec(
    repo: RepoId,
    spec: DiffSpecArg,
    engine: State<GixEngine>,
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
fn blame(
    repo: RepoId,
    path: String,
    at: Option<String>,
    engine: State<GixEngine>,
) -> Result<Blame, String> {
    let at = at.map(Oid::from);
    engine
        .blame(&repo, &path, at.as_ref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn file_history(
    repo: RepoId,
    path: String,
    engine: State<GixEngine>,
) -> Result<Vec<CommitMeta>, String> {
    engine.file_history(&repo, &path).map_err(|e| e.to_string())
}

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
fn status(repo: RepoId, engine: State<GixEngine>) -> Result<WorkingTree, String> {
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

/// Commit the staged changes, or amend the tip when `amend` is set. Returns the
/// new commit Oid.
#[tauri::command]
fn commit(
    repo: RepoId,
    message: String,
    amend: bool,
    engine: State<GixEngine>,
) -> Result<Oid, String> {
    engine
        .commit(&repo, &message, &CommitOpts { amend })
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

/// Clone `url` into `dest` via system git (ADR-0003 shell-out tier), streaming
/// git's progress lines to the frontend as `clone-progress` events, and open
/// the result.
#[tauri::command]
fn clone_repo(
    url: String,
    dest: String,
    app: tauri::AppHandle,
    engine: State<GixEngine>,
) -> Result<RepoId, String> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use tauri::Emitter;

    let mut child = Command::new("git")
        .args(["clone", "--progress", &url, &dest])
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
/// frontend as `fetch-progress` events. Auth is the system git's (ADR-0006).
#[tauri::command]
fn fetch(
    repo: RepoId,
    remote: Option<String>,
    app: tauri::AppHandle,
    engine: State<GixEngine>,
) -> Result<(), String> {
    use tauri::Emitter;
    let mut emit = |line: &str| {
        let _ = app.emit("fetch-progress", line.to_string());
    };
    engine
        .fetch(&repo, remote.as_deref(), &mut emit)
        .map_err(|e| e.to_string())
}

/// Pull (fetch + integrate) from `remote`/`branch`, or the configured upstream.
/// Progress streams as `fetch-progress` events.
#[tauri::command]
fn pull(
    repo: RepoId,
    remote: Option<String>,
    branch: Option<String>,
    app: tauri::AppHandle,
    engine: State<GixEngine>,
) -> Result<(), String> {
    use tauri::Emitter;
    let mut emit = |line: &str| {
        let _ = app.emit("fetch-progress", line.to_string());
    };
    engine
        .pull(&repo, remote.as_deref(), branch.as_deref(), &mut emit)
        .map_err(|e| e.to_string())
}

/// Push the current branch to `remote`/`branch`. `set_upstream` records the
/// tracking ref; `force` allows a non-fast-forward update. Progress streams as
/// `push-progress` events; rejections surface git's message verbatim.
#[tauri::command]
fn push(
    repo: RepoId,
    remote: Option<String>,
    branch: Option<String>,
    set_upstream: bool,
    force: bool,
    app: tauri::AppHandle,
    engine: State<GixEngine>,
) -> Result<(), String> {
    use tauri::Emitter;
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
            &mut emit,
        )
        .map_err(|e| e.to_string())
}

/// How far the current branch is ahead/behind its upstream (`None` if untracked).
#[tauri::command]
fn ahead_behind(
    repo: RepoId,
    engine: State<GixEngine>,
) -> Result<Option<lady_proto::AheadBehind>, String> {
    engine.ahead_behind(&repo).map_err(|e| e.to_string())
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

#[tauri::command]
fn merge(
    repo: RepoId,
    source: String,
    fast_forward: String,
    commit_message: Option<String>,
    engine: State<GixEngine>,
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
fn cherry_pick(
    repo: RepoId,
    oid: String,
    engine: State<GixEngine>,
) -> Result<ApplyOutcome, String> {
    engine
        .cherry_pick(&repo, &Oid::from(oid))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn revert(repo: RepoId, oid: String, engine: State<GixEngine>) -> Result<ApplyOutcome, String> {
    engine
        .revert(&repo, &Oid::from(oid))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn sequencer_abort(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
    engine.sequencer_abort(&repo).map_err(|e| e.to_string())
}

#[tauri::command]
fn rebase(
    repo: RepoId,
    branch: String,
    onto: String,
    engine: State<GixEngine>,
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
}

/// Persisted user settings (recent repos + their groups).
#[derive(Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub recent: Vec<RecentRepo>,
}

/// Path to `settings.toml` in the platform config dir (via `directories`).
fn settings_file() -> Result<std::path::PathBuf, String> {
    let dirs = directories::ProjectDirs::from("dev", "Lady", "Lady")
        .ok_or_else(|| "could not resolve a config directory".to_string())?;
    Ok(dirs.config_dir().join("settings.toml"))
}

#[tauri::command]
fn load_settings() -> Result<Settings, String> {
    let path = settings_file()?;
    match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).map_err(|e| e.to_string()),
        // Missing file → first run; return defaults.
        Err(_) => Ok(Settings::default()),
    }
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    let path = settings_file()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = toml::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .manage(GixEngine::new())
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
            fetch,
            pull,
            push,
            ahead_behind,
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
            rebase_interactive,
            rebase_continue,
            rebase_skip,
            rebase_range,
            clone_repo,
            load_settings,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

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
