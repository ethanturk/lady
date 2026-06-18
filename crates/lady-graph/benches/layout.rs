//! Graph-layout benchmark (PH6-001).
//!
//! Measures `lady_graph::layout` over a large seeded synthetic history. This is
//! the pure hot path behind the commit-graph canvas: the budget is that a
//! single screen of rows lays out far inside the 16ms/frame (60fps) budget, and
//! that a full page (a few thousand rows) stays well under the ~1s first-paint
//! budget (PLAN.md §8.9).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use lady_fixtures::synthetic_commits;
use lady_graph::layout;

fn bench_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_layout");
    for &n in &[100usize, 1_000, 10_000] {
        let commits = synthetic_commits(n, 0x1ADD_u64 ^ n as u64);
        group.bench_with_input(BenchmarkId::from_parameter(n), &commits, |b, commits| {
            b.iter(|| layout(std::hint::black_box(commits)));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_layout);
criterion_main!(benches);
