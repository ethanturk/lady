//! Merge conflict detection integration tests.

use super::*;
use lady_git::MergeOpts;
use lady_proto::MergeOutcome;

#[test]
fn test_merge_conflict_detection() {
    let dir = tmpdir();
    let engine = GixEngine::new();
    let repo_id = init_repo(&dir, &engine);

    // Create initial commit on main
    let file_path = dir.join("test.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").expect("write failed");
    commit(&dir, "Initial commit", &["test.txt"]);

    // Create feature branch (pointing to initial commit)
    create_branch(&dir, "feature");

    // Modify on feature first
    checkout(&dir, "feature");
    std::fs::write(&file_path, "line1\nFEATURE CHANGE\nline3\n").expect("write failed");
    commit(&dir, "Feature change", &["test.txt"]);

    // Switch to main and modify the same lines
    checkout(&dir, "main");
    std::fs::write(&file_path, "line1\nMAIN CHANGE\nline3\n").expect("write failed");
    commit(&dir, "Main change", &["test.txt"]);

    // Try to merge feature into main - should conflict
    let outcome = engine
        .merge(&repo_id, "feature", &MergeOpts::default())
        .expect("merge command should complete (even with conflict)");

    // Check for conflict state - merge should return Conflicts variant
    assert!(
        matches!(outcome, MergeOutcome::Conflicts(_)),
        "Expected merge conflict but got {:?}",
        outcome
    );

    cleanup(dir);
}
