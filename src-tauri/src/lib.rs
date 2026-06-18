use lady_git::{GitEngine, GixEngine};
use lady_proto::{RefInfo, RepoId};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
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

pub fn run() {
    tauri::Builder::default()
        .manage(GixEngine::new())
        .invoke_handler(tauri::generate_handler![app_info, open_repo, list_refs])
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
        std::fs::write(p.join("f.txt"), "x").expect("write fixture");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn command_open_and_list_refs() {
        let dir = fixture();
        let engine = GixEngine::new();
        // Exercises the same logic as the open_repo and list_refs commands.
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
}
