//! Integration tests for GitEngine using real Git repositories.
//!
//! These tests exercise the hybrid engine (gix + system git) with actual
//! Git workflows to verify consistency and catch regressions.

mod basic_ops;
mod branch_merge;
mod conflicts;
mod worktrees;

use lady_git::{GitEngine, GixEngine};
use lady_proto::RepoId;
use std::path::PathBuf;
use std::process::Command;

/// Create a temporary directory for tests.
fn tmpdir() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("lady-test-{}", timestamp))
}

/// Initialize a real git repo using system git and open with engine.
fn init_repo(path: &PathBuf, engine: &GixEngine) -> RepoId {
    // Create the directory first
    std::fs::create_dir_all(path).expect("failed to create temp dir");

    Command::new("git")
        .args(["init", "-q"])
        .current_dir(path)
        .output()
        .expect("git init failed");

    // Configure user for commits
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .output()
        .expect("git config email failed");

    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .output()
        .expect("git config name failed");

    // Open with engine to register the repo
    engine.open(path).expect("engine.open failed")
}

/// Create a commit using system git.
fn commit(path: &PathBuf, message: &str, files: &[&str]) -> String {
    for file in files {
        Command::new("git")
            .args(["add", file])
            .current_dir(path)
            .output()
            .expect("git add failed");
    }

    let output = Command::new("git")
        .args(["commit", "-q", "-m", message])
        .current_dir(path)
        .output()
        .expect("git commit failed");

    if !output.status.success() {
        panic!("commit failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    // Return the commit OID
    String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .expect("rev-parse failed")
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string()
}

/// Create a test branch using system git.
fn create_branch(path: &PathBuf, name: &str) {
    Command::new("git")
        .args(["branch", name])
        .current_dir(path)
        .output()
        .expect("git branch failed");
}

/// Switch to a branch using system git.
fn checkout(path: &PathBuf, branch: &str) {
    Command::new("git")
        .args(["checkout", "-q", branch])
        .current_dir(path)
        .output()
        .expect("git checkout failed");
}

/// Clean up test directory.
fn cleanup(path: PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}
