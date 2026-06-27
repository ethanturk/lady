//! Branch and merge operation integration tests.

use super::*;
use lady_git::{GraphQuery, MergeOpts};
use lady_proto::MergeOutcome;

#[test]
fn test_create_and_list_branches() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    // Create initial commit
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "hello\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Create a branch
    create_branch(&dir, "feature");

    // List refs
    let refs = engine.list_refs(&repo_id).expect("list_refs failed");

    let branch_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.kind == lady_proto::RefKind::Branch)
        .collect();

    assert!(branch_refs
        .iter()
        .any(|r| r.name == "main" || r.name == "master"));
    assert!(branch_refs.iter().any(|r| r.name == "feature"));

    cleanup(dir);
}

#[test]
fn test_merge_fast_forward() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

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
    let outcome = engine
        .merge(&repo_id, "feature", &MergeOpts::default())
        .expect("merge failed");

    // Verify we got a merge result (either fast-forward or merge commit)
    assert!(matches!(
        outcome,
        MergeOutcome::FastForwarded | MergeOutcome::Merged(_)
    ));

    // Verify commit is on main
    let commits = engine
        .walk_log(&repo_id, GraphQuery::default())
        .expect("walk_log failed");

    assert!(commits.iter().any(|c| c.summary == "Feature commit"));

    cleanup(dir);
}
