//! `lady-diff` — text diff engine (ADR-0003: pure read, no git-library dep).
//!
//! Computes line-level diffs using `imara-diff` (Histogram algorithm).
//! Returns [`DiffHunk`] slices compatible with `lady-proto`.  The git-object
//! retrieval (opening repos, resolving blobs) lives in `lady-git`; this crate
//! is purely the text-diff layer so it can be tested in isolation.

use std::ops::Range;

use lady_proto::{DiffHunk, DiffLine, LineKind};

/// Number of context lines around each change block.
const CONTEXT: u32 = 3;

/// Compute line-level hunks between `old_text` and `new_text`.
///
/// Returns an empty vec when the two texts are identical.
/// Text is split on `\n`; a trailing newline does not produce an extra blank line.
pub fn text_diff(old_text: &str, new_text: &str) -> Vec<DiffHunk> {
    use imara_diff::{diff, intern::InternedInput, Algorithm};

    // Collect raw changed ranges (0-based line indices, exclusive end).
    let input = InternedInput::new(old_text, new_text);
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();

    struct Collector(Vec<(Range<u32>, Range<u32>)>);
    impl imara_diff::Sink for Collector {
        type Out = Vec<(Range<u32>, Range<u32>)>;
        fn process_change(&mut self, before: Range<u32>, after: Range<u32>) {
            self.0.push((before, after));
        }
        fn finish(self) -> Self::Out {
            self.0
        }
    }

    let changes = diff(Algorithm::Histogram, &input, Collector(vec![]));
    if changes.is_empty() {
        return Vec::new();
    }

    build_hunks(&old_lines, &new_lines, &changes, CONTEXT)
}

/// Build `DiffHunk` structures from raw `(old_range, new_range)` change blocks.
fn build_hunks(
    old_lines: &[&str],
    new_lines: &[&str],
    changes: &[(Range<u32>, Range<u32>)],
    context: u32,
) -> Vec<DiffHunk> {
    let mut hunks: Vec<DiffHunk> = Vec::new();

    // Merge nearby change regions that fall within `context` lines of each other,
    // then expand each merged region by `context` lines for display.
    let mut i = 0;
    while i < changes.len() {
        let (old_lo, mut old_hi) = (changes[i].0.start, changes[i].0.end);
        let (new_lo, mut new_hi) = (changes[i].1.start, changes[i].1.end);
        let mut j = i + 1;
        // Merge with subsequent changes that are close enough.
        while j < changes.len() {
            let next_old_lo = changes[j].0.start;
            if next_old_lo.saturating_sub(old_hi) <= context * 2 {
                old_hi = changes[j].0.end;
                new_hi = changes[j].1.end;
                j += 1;
            } else {
                break;
            }
        }

        // Expand by context, clamped to file bounds.
        let old_start = old_lo.saturating_sub(context);
        let new_start = new_lo.saturating_sub(context);
        let old_end = (old_hi + context).min(old_lines.len() as u32);
        let new_end = (new_hi + context).min(new_lines.len() as u32);

        // Build the DiffLine list by walking the merged range and marking each line.
        let mut lines: Vec<DiffLine> = Vec::new();
        let mut op = old_start; // old-file cursor
        let mut np = new_start; // new-file cursor

        // Walk through the change sub-ranges that fall in [old_start, old_end).
        for (old_r, new_r) in &changes[i..j] {
            // Context lines before this change block.
            while op < old_r.start && op < old_end {
                lines.push(DiffLine {
                    kind: LineKind::Context,
                    content: old_lines[op as usize].to_owned(),
                });
                op += 1;
                np += 1;
            }
            // Deleted lines.
            while op < old_r.end && op < old_end {
                lines.push(DiffLine {
                    kind: LineKind::Deleted,
                    content: old_lines[op as usize].to_owned(),
                });
                op += 1;
            }
            // Added lines.
            while np < new_r.end && np < new_end {
                lines.push(DiffLine {
                    kind: LineKind::Added,
                    content: new_lines[np as usize].to_owned(),
                });
                np += 1;
            }
        }
        // Trailing context lines after all changes in the merged block.
        while op < old_end {
            lines.push(DiffLine {
                kind: LineKind::Context,
                content: old_lines[op as usize].to_owned(),
            });
            op += 1;
        }

        // Count old/new lines for the hunk header.
        let old_count = lines.iter().filter(|l| l.kind != LineKind::Added).count() as u32;
        let new_count = lines.iter().filter(|l| l.kind != LineKind::Deleted).count() as u32;

        hunks.push(DiffHunk {
            old_start: old_start + 1, // 1-indexed
            old_lines: old_count,
            new_start: new_start + 1,
            new_lines: new_count,
            lines,
        });

        i = j;
    }

    hunks
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lady_proto::LineKind;

    #[test]
    fn identical_texts_produce_no_hunks() {
        assert!(text_diff("foo\nbar\n", "foo\nbar\n").is_empty());
    }

    #[test]
    fn single_line_addition() {
        let old = "a\nb\n";
        let new = "a\nb\nc\n";
        let hunks = text_diff(old, new);
        assert_eq!(hunks.len(), 1);
        let added: Vec<_> = hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Added)
            .collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].content, "c");
    }

    #[test]
    fn single_line_deletion() {
        let old = "a\nb\nc\n";
        let new = "a\nc\n";
        let hunks = text_diff(old, new);
        assert_eq!(hunks.len(), 1);
        let deleted: Vec<_> = hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Deleted)
            .collect();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].content, "b");
    }

    #[test]
    fn hunks_contain_context_lines() {
        // 10 unchanged lines, then a change: context lines should appear.
        let old: String = (1..=10).map(|i| format!("line{i}\n")).collect();
        let new: String = (1..=10)
            .map(|i| {
                if i == 5 {
                    "CHANGED\n".to_owned()
                } else {
                    format!("line{i}\n")
                }
            })
            .collect();
        let hunks = text_diff(&old, &new);
        assert_eq!(hunks.len(), 1, "single hunk expected");
        let kinds: Vec<LineKind> = hunks[0].lines.iter().map(|l| l.kind).collect();
        assert!(
            kinds.contains(&LineKind::Context),
            "should have context lines"
        );
        assert!(
            kinds.contains(&LineKind::Deleted),
            "should have a deleted line"
        );
        assert!(
            kinds.contains(&LineKind::Added),
            "should have an added line"
        );
    }

    #[test]
    fn two_distant_changes_produce_two_hunks() {
        let n = 20;
        let old: String = (1..=n).map(|i| format!("line{i}\n")).collect();
        let new: String = (1..=n)
            .map(|i| match i {
                2 => "CHANGE_A\n".to_owned(),
                18 => "CHANGE_B\n".to_owned(),
                _ => format!("line{i}\n"),
            })
            .collect();
        let hunks = text_diff(&old, &new);
        assert_eq!(hunks.len(), 2, "distant changes should produce two hunks");
    }

    #[test]
    fn hunk_line_counts_are_correct() {
        // Replace 1 line with 2 lines.
        let old = "a\nb\nc\n";
        let new = "a\nX\nY\nc\n";
        let hunks = text_diff(old, new);
        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        let del = h
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Deleted)
            .count() as u32;
        let add = h.lines.iter().filter(|l| l.kind == LineKind::Added).count() as u32;
        assert_eq!(del, 1, "one deleted line");
        assert_eq!(add, 2, "two added lines");
        // old_lines = context + deleted = context + 1; new_lines = context + added = context + 2
        assert_eq!(
            h.old_lines,
            h.new_lines - 1 + del - add + del,
            "old_lines = context+del"
        );
    }
}
