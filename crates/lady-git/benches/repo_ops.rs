//! Repository-ops benchmark (PH6-001).
//!
//! Measures the two reads on the hot refresh path against a real, seeded
//! synthetic repository built with system git (offline, reproducible):
//! - `walk_log` — one page of history (the commit list / graph source).
//! - `status` — the working-tree scan behind the Changes view.
//!
//! Budgets (PLAN.md §8.9): status refresh < ~100ms on a warm cache; a first
//! page of log walks well inside the ~1s cold-open budget. The repo is built
//! once in setup (not measured); only the per-call cost is timed.
//!
//! If `git` is unavailable the benches register as no-ops so the harness still
//! compiles and runs on a bare machine.

use criterion::{criterion_group, criterion_main, Criterion};
use lady_fixtures::build_synthetic_repo;
use lady_git::{GitEngine, GixEngine, GraphQuery};

const COMMITS: usize = 512;
const SEED: u64 = 0xC0FFEE;

fn bench_repo_ops(c: &mut Criterion) {
    let Some(repo) = build_synthetic_repo(COMMITS, SEED) else {
        // No system git — register placeholder benches so the harness still
        // runs and the CI job stays green.
        c.bench_function("walk_log/skipped_no_git", |b| b.iter(|| ()));
        c.bench_function("status/skipped_no_git", |b| b.iter(|| ()));
        return;
    };

    let engine = GixEngine::new();
    let id = engine.open(repo.path()).expect("open synthetic repo");

    c.bench_function("walk_log/512", |b| {
        b.iter(|| {
            engine
                .walk_log(
                    std::hint::black_box(&id),
                    GraphQuery {
                        start: None,
                        limit: COMMITS,
                    },
                )
                .expect("walk_log")
        });
    });

    c.bench_function("status/dirty", |b| {
        b.iter(|| engine.status(std::hint::black_box(&id)).expect("status"));
    });
}

criterion_group!(benches, bench_repo_ops);
criterion_main!(benches);
