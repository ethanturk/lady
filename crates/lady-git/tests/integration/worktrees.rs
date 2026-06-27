//! Worktree operation integration tests.

use super::*;
use lady_git::GitEngine;

#[test]
fn test_worktree_create_and_list() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    // Create initial commit
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Create worktree using system git
    let worktree_path = dir.join("feature-worktree");
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            worktree_path.to_str().unwrap(),
            "-b",
            "feature",
        ])
        .current_dir(&dir)
        .output()
        .expect("worktree add failed");

    if !output.status.success() {
        panic!(
            "worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // List worktrees
    let worktrees = engine
        .list_worktrees(&repo_id)
        .expect("list_worktrees failed");

    assert!(
        !worktrees.is_empty(),
        "Expected at least 1 worktree"
    );

    cleanup(worktree_path);
    cleanup(dir);
}
