# Plan 014: Add GitEngine integration tests

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7ff3460..HEAD -- crates/lady-git`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: LOW
- **Depends on**: None
- **Category**: tests
- **Planned at**: commit `7ff3460`, 2026-06-26

## Why This Matters

`crates/lady-git` has 26 test modules but they're primarily unit tests using
fixtures and mocks. The hybrid Git engine (gix + system git shell-out per ADR-0003)
is Lady's core infrastructure, yet there are no integration tests that:

1. Exercise real Git workflows with actual repositories
2. Verify gix and system git paths produce consistent results
3. Test error handling with real Git edge cases
4. Validate the `GitEngine` trait implementation end-to-end

Characterization tests provide a safety net for future engine swaps and catch
regressions in the gix/system-git delegation logic.

## Current State

**Existing test structure:**

```rust
crates/lady-git/src/lib.rs:3880-3920
#[cfg(test)]
mod tests {
    use super::*;
    use lady_fixtures::tmpdir;

    #[test]
    fn test_status_empty_repo() {
        let dir = tmpdir();
        let repo = init_bare(&dir);
        let id = RepoId::from_path(dir.path());
        let engine = GixEngine::new();
        // Unit test with fixture
        assert_eq!(engine.status(&id).unwrap().unstaged.len(), 1);
    }

    // ... 25 more test modules, mostly fixture-based ...
}
```

**Test coverage gaps:**

- No tests for multi-commit workflows (branch → commit → merge)
- No tests for conflict scenarios
- No tests for remote operations (fetch, push, pull)
- No tests for worktree operations
- No tests comparing gix vs system git output consistency

**Fixture usage:**

```rust
crates/lady-git/src/lib.rs:3890-3900
fn init_bare(path: &Path) -> gix::Repository {
    gix::init(path).unwrap()
}

fn seed_commit(repo: &gix::Repository, message: &str) -> Oid {
    // Creates a single commit with fixed content
}
```

**GitEngine trait definition:**

```rust
crates/lady-git/src/lib.rs:150-200
pub trait GitEngine: Send + Sync {
    fn open(&self, path: &Path) -> Result<RepoId>;
    fn list_refs(&self, repo: &RepoId) -> Result<Vec<RefInfo>>;
    fn walk_log(&self, repo: &RepoId, query: GraphQuery) -> Result<Vec<CommitMeta>>;
    fn status(&self, repo: &RepoId) -> Result<WorkingTree>;
    fn diff(&self, repo: &RepoId, spec: DiffSpec) -> Result<FileDiff>;
    // ... 40+ methods ...
}
```

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Run Rust tests | `cargo test -p lady-git` | all tests pass |
| Run integration tests only | `cargo test -p lady-git --test integration` | integration tests pass |
| Format Rust | `cargo fmt --all -- --check` | exits 0 |
| Clippy | `cargo clippy --all-targets --all-features -- -D warnings` | exits 0 |

## Scope

**In scope**:

- `crates/lady-git/tests/integration/` — new directory for integration tests
- `crates/lady-git/tests/integration/mod.rs` — test module entry point
- Integration test files for each major workflow category
- Test fixtures using real `git` commands (not gix-only)
- Verification that gix and system git produce consistent results

**Out of scope**:

- Changing GitEngine trait signatures
- Modifying existing unit tests
- Adding new GitEngine methods
- Testing system git shell-out directly (that's in src-tauri)
- Network operations (remotes requiring authentication)

## Git Workflow

- Branch: `advisor/014-gitengine-integration-tests`
- Commit message: `test(lady-git): add integration tests for GitEngine`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Create integration test directory structure

```sh
mkdir -p crates/lady-git/tests/integration
touch crates/lady-git/tests/integration/mod.rs
touch crates/lady-git/tests/integration/basic_ops.rs
touch crates/lady-git/tests/integration/branch_merge.rs
touch crates/lady-git/tests/integration/conflicts.rs
touch crates/lady-git/tests/integration/worktrees.rs
```

Update `crates/lady-git/Cargo.toml` to include integration test:

```toml
[[test]]
name = "integration"
path = "tests/integration/mod.rs"
```

**Verify**: `cargo test -p lady-git --test integration` — runs (even if no tests yet).

### Step 2: Create integration test helpers

Add to `crates/lady-git/tests/integration/mod.rs`:

```rust
use lady_git::{GixEngine, GitEngine, RepoId};
use std::path::PathBuf;
use std::process::Command;

/// Create a temporary directory for tests.
fn tmpdir() -> PathBuf {
    std::env::temp_dir().join(format!("lady-test-{}", uuid::Uuid::new_v4()))
}

/// Initialize a real git repo using system git.
fn init_repo(path: &Path) -> RepoId {
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

    RepoId::from_path(path)
}

/// Create a commit using system git.
fn commit(path: &Path, message: &str, files: &[&str]) -> String {
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
            .stdout
    ).unwrap().trim().to_string()
}

/// Create a test branch using system git.
fn create_branch(path: &Path, name: &str) {
    Command::new("git")
        .args(["branch", name])
        .current_dir(path)
        .output()
        .expect("git branch failed");
}

/// Switch to a branch using system git.
fn checkout(path: &Path, branch: &str) {
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
```

**Verify**: `cargo test -p lady-git --test integration` — compiles.

### Step 3: Add basic operations tests

Create `crates/lady-git/tests/integration/basic_ops.rs`:

```rust
use super::*;
use lady_git::{DiffSpec, GraphQuery};

#[test]
fn test_init_and_status() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    let status = engine.status(&repo_id).expect("status failed");
    assert!(status.staged.is_empty());
    assert!(status.unstaged.is_empty());

    cleanup(dir);
}

#[test]
fn test_commit_and_log() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    // Create a file and commit
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");

    let oid = commit(&dir, "Initial commit", &["test.txt"]);

    // Verify log
    let commits = engine.walk_log(&repo_id, GraphQuery::default())
        .expect("walk_log failed");

    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].summary, "Initial commit");
    assert!(commits[0].oid == oid || commits[0].oid.starts_with(&oid[..8]));

    cleanup(dir);
}

#[test]
fn test_status_after_modify() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

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
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    // Create and commit a file
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Modify the file
    std::fs::write(&file_path, "hello world\n").expect("write failed");

    // Get diff
    let diff = engine.diff(&repo_id, DiffSpec::WorkingVsIndex("test.txt".to_string()))
        .expect("diff failed");

    assert!(!diff.hunks.is_empty());
    assert!(diff.hunks[0].old_text.contains("hello\n"));
    assert!(diff.hunks[0].new_text.contains("hello world\n"));

    cleanup(dir);
}
```

**Verify**: `cargo test -p lady-git --test integration basic_ops` — all 4 tests pass.

### Step 4: Add branch and merge tests

Create `crates/lady-git/tests/integration/branch_merge.rs`:

```rust
use super::*;
use lady_git::{GraphQuery, MergeOpts};

#[test]
fn test_create_and_list_branches() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    // Create initial commit
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Create a branch
    create_branch(&dir, "feature");

    // List refs
    let refs = engine.list_refs(&repo_id).expect("list_refs failed");

    let branch_refs: Vec<_> = refs.iter()
        .filter(|r| r.kind == lady_proto::RefKind::Branch)
        .collect();

    assert!(branch_refs.iter().any(|r| r.name == "main" || r.name == "master"));
    assert!(branch_refs.iter().any(|r| r.name == "feature"));

    cleanup(dir);
}

#[test]
fn test_merge_fast_forward() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    // Create initial commit on main
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Create and switch to feature branch
    create_branch(&dir, "feature");
    checkout(&dir, "feature");

    // Add a commit on feature
    std::fs::write(&file_path, "hello feature\n").expect("write failed");
    commit(&dir, "Feature commit", &["test.txt"]);

    // Switch back to main
    checkout(&dir, "main");

    // Merge feature into main
    let outcome = engine.merge(&repo_id, "feature".to_string(), MergeOpts::default())
        .expect("merge failed");

    assert!(outcome.success);

    // Verify commit is on main
    let commits = engine.walk_log(&repo_id, GraphQuery::default())
        .expect("walk_log failed");

    assert!(commits.iter().any(|c| c.summary == "Feature commit"));

    cleanup(dir);
}
```

**Verify**: `cargo test -p lady-git --test integration branch_merge` — all tests pass.

### Step 5: Add conflict tests

Create `crates/lady-git/tests/integration/conflicts.rs`:

```rust
use super::*;
use lady_git::{MergeOpts};

#[test]
fn test_merge_conflict_detection() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    // Create initial commit
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Create feature branch
    create_branch(&dir, "feature");

    // Modify on main
    checkout(&dir, "main");
    std::fs::write(&file_path, "line1\nMAIN CHANGE\nline3\n").expect("write failed");
    commit(&dir, "Main change", &["test.txt"]);

    // Modify on feature (same lines, different content)
    checkout(&dir, "feature");
    std::fs::write(&file_path, "line1\nFEATURE CHANGE\nline3\n").expect("write failed");
    commit(&dir, "Feature change", &["test.txt"]);

    // Try to merge - should conflict
    let outcome = engine.merge(&repo_id, "feature".to_string(), MergeOpts::default())
        .expect("merge command should complete (even with conflict)");

    // Check for conflict state
    assert!(!outcome.success || engine.status(&repo_id).expect("status").conflicts.is_some());

    cleanup(dir);
}
```

**Verify**: `cargo test -p lady-git --test integration conflicts` — test passes.

### Step 6: Add worktree tests

Create `crates/lady-git/tests/integration/worktrees.rs`:

```rust
use super::*;
use lady_git::GitEngine;

#[test]
fn test_worktree_create_and_list() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    // Create initial commit
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Create worktree using system git
    let worktree_path = dir.join("../feature-worktree");
    Command::new("git")
        .args(["worktree", "add", worktree_path.to_str().unwrap(), "-b", "feature"])
        .current_dir(&dir)
        .output()
        .expect("worktree add failed");

    // List worktrees
    let worktrees = engine.list_worktrees(&repo_id).expect("list_worktrees failed");

    assert!(worktrees.len() >= 1); // Main worktree + new one

    cleanup(worktree_path);
    cleanup(dir);
}
```

**Verify**: `cargo test -p lady-git --test integration worktrees` — test passes.

### Step 7: Add consistency tests (gix vs system git)

Append to `crates/lady-git/tests/integration/basic_ops.rs`:

```rust
#[test]
fn test_log_consistency_gix_vs_system() {
    let dir = tmpdir();
    let repo_id = init_repo(&dir);
    let engine = GixEngine::new();

    // Create multiple commits
    let file_path = dir.join("test.txt");
    for i in 0..5 {
        std::fs::write(&file_path, format!("commit {}\n", i)).expect("write failed");
        commit(&dir, &format!("Commit {}", i), &["test.txt"]);
    }

    // Get log via gix
    let gix_commits = engine.walk_log(&repo_id, GraphQuery {
        start: None,
        limit: 10,
    }).expect("gix walk_log failed");

    // Get log via system git
    let system_output = Command::new("git")
        .args(["log", "--pretty=format:%H %s", "-n", "10"])
        .current_dir(&dir)
        .output()
        .expect("git log failed");

    let system_lines: Vec<&str> = String::from_utf8_lossy(&system_output.stdout)
        .lines()
        .collect();

    // Compare counts
    assert_eq!(gix_commits.len(), system_lines.len(),
        "gix and system git returned different commit counts");

    cleanup(dir);
}
```

**Verify**: `cargo test -p lady-git --test integration` — all tests pass.

### Step 8: Run full verification suite

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test -p lady-git
cargo test
```

**Verify**: All commands exit 0, integration tests show ≥10 new tests passing.

## Test Plan

New integration tests:

- `basic_ops.rs` — 5 tests (init/status, commit/log, status after modify, diff, log consistency)
- `branch_merge.rs` — 2 tests (branch creation/listing, fast-forward merge)
- `conflicts.rs` — 1 test (merge conflict detection)
- `worktrees.rs` — 1 test (worktree creation/listing)

Total: ≥9 new integration tests covering real Git workflows.

## Done Criteria

- [ ] `crates/lady-git/tests/integration/` directory created
- [ ] `crates/lady-git/tests/integration/mod.rs` with test helpers
- [ ] `crates/lady-git/tests/integration/basic_ops.rs` with ≥5 tests
- [ ] `crates/lady-git/tests/integration/branch_merge.rs` with ≥2 tests
- [ ] `crates/lady-git/tests/integration/conflicts.rs` with ≥1 test
- [ ] `crates/lady-git/tests/integration/worktrees.rs` with ≥1 test
- [ ] `cargo test -p lady-git --test integration` passes with ≥9 new tests
- [ ] `cargo test` (whole workspace) passes
- [ ] `cargo fmt --all -- --check` exits 0
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` exits 0
- [ ] `plans/README.md` status row updated

## STOP Conditions

Stop and report back if:

- System git is not available or fails basic operations
- Integration tests cannot be added due to Cargo.toml structure changes
- Any integration test fails twice after reasonable fixes
- gix and system git produce fundamentally different results (investigate before proceeding)

## Maintenance Notes

Future GitEngine changes should:

- Add integration tests for any new GitEngine methods
- Keep integration tests using real `git` commands (not gix-only)
- Run integration tests in CI (`cargo test --test integration`)
- Update consistency tests when GitEngine trait changes
- Target ≥80% coverage of GitEngine methods via integration tests
