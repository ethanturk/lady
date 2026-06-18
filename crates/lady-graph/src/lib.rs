//! `lady-graph` — commit-graph lane layout engine (ADR-0005).
//!
//! Takes a topologically-ordered slice of [`CommitMeta`] (newest first, as
//! returned by `GixEngine::walk_log`) and assigns each commit a horizontal
//! lane and a set of line-segment edges, producing [`GraphRow`] data ready
//! for the canvas renderer.

use lady_proto::{CommitMeta, Oid};

/// A single line segment connecting two adjacent rows of the commit graph.
///
/// `from_lane` is the segment's position at the **bottom** of this row;
/// `to_lane` is its position at the **top** of the next row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Lane index in this row where the segment exits.
    pub from_lane: usize,
    /// Lane index in the next row where the segment enters.
    pub to_lane: usize,
}

/// All rendering data for one row of the commit graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRow {
    /// This commit's OID.
    pub oid: Oid,
    /// Horizontal lane (0-indexed column) where the commit circle is drawn.
    pub lane: usize,
    /// Total number of active lanes needed to render this row (graph width).
    pub num_lanes: usize,
    /// Line segments running from the bottom of this row to the top of the
    /// next. Each `(from_lane, to_lane)` is one continuous thread.
    pub edges: Vec<Edge>,
    /// Reference names pointing at this commit (e.g. "main", "HEAD", "v1.0").
    /// `layout()` always returns empty vecs; callers populate this field.
    pub refs: Vec<String>,
}

/// Active-lane state between layout pages (opaque to the caller).
///
/// Pass this back to [`layout_continuation`] to correctly extend the graph
/// when a second batch of commits arrives.  `None` entries are empty lane
/// slots that can be recycled.
pub type ActiveLanes = Vec<Option<Oid>>;

/// Assign lanes and route edges for a topologically-ordered commit slice.
///
/// The slice must be in topological order (newest first), as produced by
/// `GixEngine::walk_log`. The output is one [`GraphRow`] per commit, in the
/// same order. Handles linear history, branches, two-parent merges, and
/// octopus merges deterministically — identical input always yields identical
/// output.
pub fn layout(commits: &[CommitMeta]) -> Vec<GraphRow> {
    let mut active: ActiveLanes = Vec::new();
    layout_inner(commits, &mut active)
}

/// Continue layout from a prior page's [`ActiveLanes`] state.
///
/// Returns the new rows together with the updated state for the next page.
/// Use when loading commits incrementally: pass the state returned by the
/// previous call so lane assignments remain consistent across pages.
pub fn layout_continuation(
    commits: &[CommitMeta],
    state: ActiveLanes,
) -> (Vec<GraphRow>, ActiveLanes) {
    let mut active = state;
    let rows = layout_inner(commits, &mut active);
    (rows, active)
}

fn layout_inner(commits: &[CommitMeta], active: &mut Vec<Option<Oid>>) -> Vec<GraphRow> {
    let mut rows = Vec::with_capacity(commits.len());

    for c in commits {
        let waiting: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                if l.as_ref() == Some(&c.oid) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        // Assign this commit to the first waiting lane, or open a fresh one.
        let my_lane = if let Some(&first) = waiting.first() {
            first
        } else {
            first_empty_or_new(active)
        };

        // ── Build the next-row active-lane state ──────────────────────────
        let mut next = active.clone();
        // Consume all lanes that were waiting for this commit.
        for &w in &waiting {
            next[w] = None;
        }
        // Assign parents.  First parent continues on my_lane; additional
        // parents get new/recycled lanes.  If a parent is already tracked
        // (fan-in convergence), skip — the existing lane covers it.
        for (pi, parent) in c.parents.iter().enumerate() {
            if lane_contains(&next, parent) {
                continue;
            }
            if pi == 0 {
                next[my_lane] = Some(parent.clone());
            } else {
                let slot = first_empty_or_new(&mut next);
                next[slot] = Some(parent.clone());
            }
        }
        while next.last() == Some(&None) {
            next.pop();
        }

        // ── Compute edges (bottom of this row → top of next row) ─────────
        let mut edges: Vec<Edge> = Vec::new();

        for (i, slot) in active.iter().enumerate() {
            let oid = match slot.as_ref() {
                Some(o) => o,
                None => continue, // dead lane — no line passes through
            };
            if oid == &c.oid {
                // Fan-in: lane i was waiting for c and converges to my_lane.
                if i != my_lane {
                    edges.push(Edge {
                        from_lane: i,
                        to_lane: my_lane,
                    });
                }
                // my_lane's outgoing edges are added in the parent loop below.
            } else {
                // Pass-through: lane continues straight to next row.
                edges.push(Edge {
                    from_lane: i,
                    to_lane: i,
                });
            }
        }

        // Lines from the commit circle to each parent's lane in the next row.
        for parent in &c.parents {
            if let Some(to) = find_lane(&next, parent) {
                edges.push(Edge {
                    from_lane: my_lane,
                    to_lane: to,
                });
            }
        }

        let num_lanes = edges
            .iter()
            .flat_map(|e| [e.from_lane, e.to_lane])
            .chain(std::iter::once(my_lane))
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);

        rows.push(GraphRow {
            oid: c.oid.clone(),
            lane: my_lane,
            num_lanes,
            edges,
            refs: Vec::new(),
        });

        *active = next;
    }

    rows
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Return the index of the first `None` slot in `lanes`.
/// If no slot is empty, push a new `None` and return its index.
fn first_empty_or_new(lanes: &mut Vec<Option<Oid>>) -> usize {
    if let Some(i) = lanes.iter().position(|l| l.is_none()) {
        i
    } else {
        lanes.push(None);
        lanes.len() - 1
    }
}

/// Return the first index in `lanes` that holds `oid`, or `None`.
fn find_lane(lanes: &[Option<Oid>], oid: &Oid) -> Option<usize> {
    lanes.iter().position(|l| l.as_ref() == Some(oid))
}

/// Return `true` if any lane in `lanes` holds `oid`.
fn lane_contains(lanes: &[Option<Oid>], oid: &Oid) -> bool {
    find_lane(lanes, oid).is_some()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lady_proto::{CommitMeta, Signature as Sig};

    /// Build an Oid from a short tag padded to 40 hex chars.
    fn o(tag: &str) -> Oid {
        Oid::from(format!("{:0<40}", tag))
    }

    /// Build a minimal CommitMeta for layout testing.
    fn c(id: &str, parents: &[&str]) -> CommitMeta {
        CommitMeta {
            oid: o(id),
            parents: parents.iter().map(|p| o(p)).collect(),
            author: Sig {
                name: "Test".into(),
                email: "t@t.com".into(),
            },
            committer: Sig {
                name: "Test".into(),
                email: "t@t.com".into(),
            },
            summary: id.into(),
            time: 0,
        }
    }

    fn e(from: usize, to: usize) -> Edge {
        Edge {
            from_lane: from,
            to_lane: to,
        }
    }

    /// Sort edges by (from, to) so tests are order-independent.
    fn sorted(mut edges: Vec<Edge>) -> Vec<Edge> {
        edges.sort_by_key(|e| (e.from_lane, e.to_lane));
        edges
    }

    // ── Linear history ───────────────────────────────────────────────────────

    #[test]
    fn linear_history() {
        // A → B → C (C is root)
        let commits = [c("A", &["B"]), c("B", &["C"]), c("C", &[])];
        let rows = layout(&commits);

        assert_eq!(rows[0].lane, 0);
        assert_eq!(sorted(rows[0].edges.clone()), vec![e(0, 0)]);

        assert_eq!(rows[1].lane, 0);
        assert_eq!(sorted(rows[1].edges.clone()), vec![e(0, 0)]);

        assert_eq!(rows[2].lane, 0);
        assert!(rows[2].edges.is_empty(), "root has no outgoing edges");
    }

    // ── Branch + two-parent merge ─────────────────────────────────────────────

    #[test]
    fn branch_and_merge() {
        // D (merge) → [C, B]
        // C → E
        // B → E
        // E (root)
        // Walk order: D, C, B, E
        let commits = [
            c("D", &["C", "B"]),
            c("C", &["E"]),
            c("B", &["E"]),
            c("E", &[]),
        ];
        let rows = layout(&commits);

        // D: merge commit, spawns two lanes.
        assert_eq!(rows[0].lane, 0, "D on lane 0");
        assert_eq!(
            sorted(rows[0].edges.clone()),
            vec![e(0, 0), e(0, 1)],
            "D: two outgoing lines to C (lane 0) and B (lane 1)"
        );

        // C: continues lane 0.
        assert_eq!(rows[1].lane, 0, "C on lane 0");
        assert_eq!(
            sorted(rows[1].edges.clone()),
            vec![e(0, 0), e(1, 1)],
            "C: lane 0→0 (to E), lane 1 passes through"
        );

        // B: on lane 1, converges back to lane 0 where E will be.
        assert_eq!(rows[2].lane, 1, "B on lane 1");
        assert_eq!(
            sorted(rows[2].edges.clone()),
            vec![e(0, 0), e(1, 0)],
            "B: lane 0 passes through, lane 1 merges to lane 0"
        );

        // E: root.
        assert_eq!(rows[3].lane, 0, "E on lane 0");
        assert!(rows[3].edges.is_empty(), "E root has no edges");
    }

    // ── Octopus merge (three parents) ────────────────────────────────────────

    #[test]
    fn octopus_merge() {
        // M → [A, B, C]
        // A, B, C each → R (root)
        // Walk order: M, A, B, C, R
        let commits = [
            c("M", &["A", "B", "C"]),
            c("A", &["R"]),
            c("B", &["R"]),
            c("C", &["R"]),
            c("R", &[]),
        ];
        let rows = layout(&commits);

        // M: spawns three lanes.
        assert_eq!(rows[0].lane, 0);
        assert_eq!(
            sorted(rows[0].edges.clone()),
            vec![e(0, 0), e(0, 1), e(0, 2)],
            "M: three outgoing lines"
        );
        assert_eq!(rows[0].num_lanes, 3);

        // A: on lane 0.
        assert_eq!(rows[1].lane, 0);

        // B: on lane 1; its parent R is already tracked, so lane 1 merges.
        assert_eq!(rows[2].lane, 1);
        assert!(
            rows[2]
                .edges
                .iter()
                .any(|e| e.from_lane == 1 && e.to_lane == 0),
            "B's lane must merge to lane 0 (where R is)"
        );

        // C: on lane 2; similarly merges to lane 0.
        assert_eq!(rows[3].lane, 2);
        assert!(
            rows[3]
                .edges
                .iter()
                .any(|e| e.from_lane == 2 && e.to_lane == 0),
            "C's lane must merge to lane 0"
        );

        // R: root.
        assert_eq!(rows[4].lane, 0);
        assert!(rows[4].edges.is_empty());
    }

    // ── Stability / determinism ───────────────────────────────────────────────

    #[test]
    fn deterministic() {
        let commits = [
            c("D", &["C", "B"]),
            c("C", &["E"]),
            c("B", &["E"]),
            c("E", &[]),
        ];
        let r1 = layout(&commits);
        let r2 = layout(&commits);
        assert_eq!(r1, r2, "layout must be deterministic");
    }

    // ── Single commit (edge case) ─────────────────────────────────────────────

    #[test]
    fn single_root_commit() {
        let commits = [c("A", &[])];
        let rows = layout(&commits);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].num_lanes, 1);
        assert!(rows[0].edges.is_empty());
    }
}
