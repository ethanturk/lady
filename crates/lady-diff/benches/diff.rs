//! Diff benchmark (PH6-001).
//!
//! Measures `lady_diff::text_diff` over large seeded synthetic file pairs. This
//! is the pure hot path behind the diff viewer; a single large file must diff
//! well inside the warm-refresh budget so opening a diff feels instant
//! (PLAN.md §8.9).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use lady_diff::text_diff;
use lady_fixtures::synthetic_text;

fn bench_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_diff");
    for &lines in &[500usize, 5_000, 20_000] {
        let (old, new) = synthetic_text(lines, 0xD1FF ^ lines as u64);
        group.bench_with_input(
            BenchmarkId::from_parameter(lines),
            &(old, new),
            |b, (old, new)| {
                b.iter(|| {
                    text_diff(
                        std::hint::black_box(old.as_str()),
                        std::hint::black_box(new.as_str()),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_diff);
criterion_main!(benches);
