# Lady — Performance budgets & benchmarks (PH6-001 / PH6-002)

Lady's responsiveness targets come from PLAN.md §8.9:

| Budget | Target |
| --- | --- |
| Cold open of a large repo → first paint | < ~1 s |
| Working-tree status refresh (warm cache) | < ~100 ms |
| Commit-graph scroll | 60 fps (≤ 16.6 ms / frame) |

We measure the **pure hot paths** behind those budgets with
[criterion](https://github.com/bheisler/criterion.rs) benchmarks over a
**seeded, offline synthetic fixture** (`crates/lady-fixtures`) so the numbers
are reproducible on any machine and in CI:

- `lady-graph` — `benches/layout.rs` → `layout()` over 100 / 1 000 / 10 000 commits
- `lady-diff` — `benches/diff.rs` → `text_diff()` over 500 / 5 000 / 20 000 lines
- `lady-git` — `benches/repo_ops.rs` → `walk_log()` + `status()` over a real
  512-commit repo built with system git

## Running

```sh
# Full run (records criterion baselines under target/criterion):
cargo bench -p lady-graph -p lady-diff -p lady-git

# Quick, non-gating run (what CI does):
cargo bench -p lady-graph -p lady-diff -p lady-git -- --quick --noplot

# Compile-only (rot check):
cargo bench --no-run -p lady-graph -p lady-diff -p lady-git
```

CI runs the quick mode in the `bench` job (`.github/workflows/ci.yml`). It is
**not a perf gate** — shared runners are too noisy to assert wall-clock budgets,
so the harness only has to compile and execute. The measured numbers below are
recorded by hand from a developer machine.

## Measured numbers

Dev machine: Apple Silicon (macOS), `cargo bench -- --quick`, release profile.
Re-run the commands above to refresh.

| Bench | Input | Time (median) | Relevant budget | Verdict |
| --- | --- | --- | --- | --- |
| `graph_layout` | 100 commits (≈ one screen) | ~14 µs | 16.6 ms/frame (60 fps) | ✅ ~1000× headroom |
| `graph_layout` | 1 000 commits | ~138 µs | first-paint < 1 s | ✅ |
| `graph_layout` | 10 000 commits | ~1.37 ms | first-paint < 1 s | ✅ |
| `text_diff` | 500 lines | ~44 µs | warm refresh | ✅ |
| `text_diff` | 5 000 lines | ~475 µs | warm refresh | ✅ |
| `text_diff` | 20 000 lines | ~2.0 ms | warm refresh | ✅ |
| `walk_log` | 512-commit page | ~22 ms | cold open < 1 s | ✅ |
| `status` | dirty 512-commit repo | ~11 ms | warm refresh < 100 ms | ✅ |

## Interpretation (PH6-002)

All hot paths sit comfortably inside budget, so **no targeted optimization was
required** — the relevant property is that they are already incremental / bounded
from earlier phases:

- **Graph layout streams rows.** `lady_graph::layout_continuation` carries
  `ActiveLanes` between pages, so scrolling/refreshing extends the graph from the
  prior state instead of re-laying-out all of history. A single screen of rows is
  the only per-frame work; 100 rows lay out in ~14 µs (≈ 0.1 % of a 60 fps frame).
- **Log walk is paged.** `walk_log` takes a bounded `GraphQuery { limit }`; the UI
  requests one page at a time rather than the whole history, so there is no
  O(history) work on the UI thread for one screen.
- **Diffs are bounded + lazy.** `text_diff` is linear-ish in file size and a
  20 000-line file diffs in ~2 ms; the diff view virtualizes rows and large /
  binary / image diffs are bounded in the viewer.
- **Status is a single warm scan.** `status` over a dirty 512-commit repo returns
  in ~11 ms, well under the 100 ms warm-refresh budget.

### Residual gaps

None at the measured sizes. The one item to watch is `walk_log` at very large
limits: cost is linear in `limit`, so the UI must keep paging (it does). If a
future profile shows a regression, optimize there first; the budget headroom
today does not justify pre-optimizing.
