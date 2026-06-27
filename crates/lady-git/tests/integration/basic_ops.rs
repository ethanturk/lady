//! Basic repository operations integration tests.

use super::*;
use lady_git::{DiffSpec, GraphQuery};

#[test]
fn test_init_and_status() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    let status = engine.status(&repo_id).expect("status failed");
    assert!(status.staged.is_empty());
    assert!(status.unstaged.is_empty());

    cleanup(dir);
}

#[test]
fn test_commit_and_log() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    // Create a file and commit
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");

    let oid = commit(&dir, "Initial commit", &["test.txt"]);

    // Verify log
    let commits = engine
        .walk_log(&repo_id, GraphQuery::default())
        .expect("walk_log failed");

    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].summary, "Initial commit");
    // Compare first 8 chars of the OID
    assert!(commits[0].oid.0.starts_with(&oid[..8]));

    cleanup(dir);
}

#[test]
fn test_status_after_modify() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    // Create and commit a file
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Modify the file
    std::fs::write(&file_path, "hello world\n").expect("write failed");

    // Verify unstaged changes
    let status = engine.status(&repo_id).expect("status failed");
    assert_eq!(status.unstaged.len(), 1);
    assert!(status.unstaged[0].path == "test.txt");

    cleanup(dir);
}

#[test]
fn test_diff_working_tree() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    // Create and commit a file
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Modify the file
    std::fs::write(&file_path, "hello world\n").expect("write failed");

    // Get diff
    let diff = engine
        .diff_spec(&repo_id, &DiffSpec::WorkingVsIndex("test.txt".to_string()))
        .expect("diff failed");

    assert!(!diff.is_empty());
    // Just verify we got some hunks back - don't check exact line content
    assert!(!diff[0].hunks.is_empty());

    cleanup(dir);
}

#[test]
fn test_log_consistency_gix_vs_system() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    // Create multiple commits
    let file_path = dir.join("test.txt");
    for i in 0..5 {
        std::fs::write(&file_path, format!("commit {}\n", i)).expect("write failed");
        commit(&dir, &format!("Commit {}", i), &["test.txt"]);
    }

    // Get log via gix
    let gix_commits = engine
        .walk_log(
            &repo_id,
            GraphQuery {
                start: None,
                limit: 10,
            },
        )
        .expect("gix walk_log failed");

    // Get log via system git
    let system_output = Command::new("git")
        .args(["log", "--pretty=format:%H %s", "-n", "10"])
        .current_dir(&dir)
        .output()
        .expect("git log failed");

    let system_output_str = String::from_utf8_lossy(&system_output.stdout);
    let system_lines: Vec<&str> = system_output_str.lines().collect();

    // Compare counts
    assert_eq!(
        gix_commits.len(),
        system_lines.len(),
        "gix and system git returned different commit counts: gix={}, system={}",
        gix_commits.len(),
        system_lines.len()
    );

    cleanup(dir);
}
