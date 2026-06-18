use lady_git::{GitEngine, GixEngine, GraphQuery};
use lady_graph::layout_continuation;
use lady_proto::{Blame, CommitMeta, FileDiff, Oid, RefInfo, RepoId, WorkingTree};
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
            blame,
            file_history,
            repo_dirty,
            list_files,
            status,
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
