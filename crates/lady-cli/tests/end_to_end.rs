//! End-to-end Phase 0 test: build a throwaway repo, then assert the CLI report
//! surfaces its refs and commit summaries via the full `lady_git` read path.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Run `git` in `dir`, asserting success. System git is permitted for
/// test-fixture setup only (ADR-0003).
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .expect("system git must be installed to run lady-cli tests");
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}

/// A throwaway repo with three commits on `main` and a `v1` tag on the tip.
fn fixture_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.name", "Lady Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    for i in 1..=3 {
        std::fs::write(p.join(format!("file{i}.txt")), format!("content {i}\n"))
            .expect("write fixture file");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", &format!("commit {i}")]);
    }
    git(p, &["tag", "v1"]);
    dir
}

#[test]
fn report_lists_refs_and_commit_summaries() {
    let dir = fixture_repo();
    let report = lady_cli::report(dir.path()).expect("build the CLI report");

    // Refs: the branch, the tag, and HEAD all appear.
    assert!(
        report.contains("main"),
        "report should list `main`:\n{report}"
    );
    assert!(
        report.contains("v1"),
        "report should list tag `v1`:\n{report}"
    );
    assert!(
        report.contains("HEAD"),
        "report should list HEAD:\n{report}"
    );

    // Commits: every fixture summary appears, newest-first.
    for summary in ["commit 1", "commit 2", "commit 3"] {
        assert!(
            report.contains(summary),
            "report should include {summary:?}:\n{report}"
        );
    }
}

#[test]
fn report_errors_on_non_repo_dir() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let err = lady_cli::report(dir.path()).expect_err("a non-repo dir must error");
    assert!(
        matches!(err, lady_git::Error::Open { .. }),
        "expected Error::Open, got {err:?}"
    );
}
