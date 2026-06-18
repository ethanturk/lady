use lady_git::{GitEngine, GixEngine, GraphQuery};
use lady_proto::{CommitMeta, Oid, RefInfo, RepoId};
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

pub fn run() {
    tauri::Builder::default()
        .manage(GixEngine::new())
        .invoke_handler(tauri::generate_handler![
            app_info, open_repo, list_refs, walk_log
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
