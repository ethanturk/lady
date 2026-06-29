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
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TMPDIR_ID: AtomicU64 = AtomicU64::new(0);

/// Create a temporary directory for tests.
fn tmpdir() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let id = NEXT_TMPDIR_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("lady-test-{}-{id}", timestamp))
}

fn git(path: &PathBuf, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("git command failed");

    if !output.status.success() {
        panic!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Initialize a real git repo using system git and open with engine.
fn init_repo(path: &PathBuf, engine: &GixEngine) -> RepoId {
    // Create the directory first
    std::fs::create_dir_all(path).expect("failed to create temp dir");

    git(path, &["init", "-q", "-b", "main"]);

    // Configure user for commits
    git(path, &["config", "user.email", "test@example.com"]);
    git(path, &["config", "user.name", "Test User"]);

    // Open with engine to register the repo
    engine.open(path).expect("engine.open failed")
}

/// Create a commit using system git.
fn commit(path: &PathBuf, message: &str, files: &[&str]) -> String {
    for file in files {
        git(path, &["add", file]);
    }

    git(path, &["commit", "-q", "-m", message]);
    git(path, &["rev-parse", "HEAD"])
}

/// Create a test branch using system git.
fn create_branch(path: &PathBuf, name: &str) {
    git(path, &["branch", name]);
}

/// Switch to a branch using system git.
fn checkout(path: &PathBuf, branch: &str) {
    git(path, &["checkout", "-q", branch]);
}

/// Clean up test directory.
fn cleanup(path: PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}
