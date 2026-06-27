//! `lady-cli` — a thin end-to-end harness over [`lady_git`] (Phase 0 EXIT).
//!
//! It opens a repository, lists its refs, and walks the first commits of
//! history to a flat list — exercising the whole read path in one shot. The
//! report-building logic lives here (not in `main`) so integration tests can
//! drive it without spawning a process.

use std::fmt::Write as _;
use std::path::Path;

use lady_git::{GitEngine, GixEngine, GraphQuery};

/// How many commits the Phase 0 report prints.
pub const LOG_LIMIT: usize = 20;

/// Open `path`, then build a human-readable report of its refs followed by the
/// first [`LOG_LIMIT`] commits (oid + summary) reachable from `HEAD`.
pub fn report(path: &Path) -> lady_git::Result<String> {
    let engine = GixEngine::new();
    let repo = engine.open(path)?;

    let mut out = String::new();

    let refs = engine.list_refs(&repo)?;
    let _ = writeln!(out, "Refs ({}):", refs.len());
    for r in &refs {
        let _ = writeln!(out, "  {:?} {} -> {}", r.kind, r.name, r.target.as_str());
        // infallible
    }

    let commits = engine.walk_log(
        &repo,
        GraphQuery {
            start: None,
            limit: LOG_LIMIT,
        },
    )?;
    let _ = writeln!(out, "\nCommits (first {LOG_LIMIT}):"); // infallible
    for c in &commits {
        let _ = writeln!(out, "  {} {}", c.oid.as_str(), c.summary); // infallible
    }

    Ok(out)
}
