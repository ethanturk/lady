//! Conflict-marker parsing for the 3-pane resolver (PLAN.md §4 lady-diff::merge).
//!
//! Parses a conflicted file's `<<<<<<< / ||||||| / ======= / >>>>>>>` markers
//! into ordered [`ConflictSegment`]s (context + [`ConflictRegion`]s), and
//! reconstructs a resolved file by taking one side of every region. This is the
//! pure-text foundation the engine drives for take-ours / take-theirs.

use lady_proto::{ConflictRegion, ConflictSegment, ParsedConflict};

/// Conflict marker prefixes (git emits exactly 7 of each marker char).
const OURS_MARK: &str = "<<<<<<<";
const BASE_MARK: &str = "|||||||";
const SEP_MARK: &str = "=======";
const THEIRS_MARK: &str = ">>>>>>>";

/// Parse a conflicted file's text into context and conflict segments.
///
/// Lines are split on `\n`; line endings are not preserved (reconstruction
/// re-emits `\n`). A region's `base` is populated only when the file carries
/// diff3-style markers (a `|||||||` section).
pub fn parse_conflicts(text: &str) -> ParsedConflict {
    let mut segments: Vec<ConflictSegment> = Vec::new();
    let mut has_base = false;
    let mut context: Vec<String> = Vec::new();

    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if !line.starts_with(OURS_MARK) {
            context.push(line.to_string());
            continue;
        }

        // Entering a conflict: flush any pending context first.
        if !context.is_empty() {
            segments.push(ConflictSegment::Context(std::mem::take(&mut context)));
        }

        let mut region = ConflictRegion {
            ours: Vec::new(),
            base: Vec::new(),
            theirs: Vec::new(),
        };
        // 0 = ours, 1 = base (diff3), 2 = theirs.
        let mut section = 0u8;
        for l in lines.by_ref() {
            if l.starts_with(THEIRS_MARK) {
                break;
            } else if l.starts_with(BASE_MARK) {
                section = 1;
                has_base = true;
            } else if l.starts_with(SEP_MARK) {
                section = 2;
            } else {
                match section {
                    0 => region.ours.push(l.to_string()),
                    1 => region.base.push(l.to_string()),
                    _ => region.theirs.push(l.to_string()),
                }
            }
        }
        segments.push(ConflictSegment::Conflict(region));
    }

    if !context.is_empty() {
        segments.push(ConflictSegment::Context(context));
    }

    ParsedConflict { segments, has_base }
}

/// Reconstruct the file content by taking one side of every conflict region.
///
/// `take_ours` chooses the `ours` side; otherwise `theirs`. Context segments
/// pass through unchanged. Every emitted line ends with `\n`.
pub fn resolve(parsed: &ParsedConflict, take_ours: bool) -> String {
    let mut out = String::new();
    for seg in &parsed.segments {
        let lines = match seg {
            ConflictSegment::Context(lines) => lines,
            ConflictSegment::Conflict(r) => {
                if take_ours {
                    &r.ours
                } else {
                    &r.theirs
                }
            }
        };
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFLICT: &str = "\
top line
<<<<<<< HEAD
our change
=======
their change
>>>>>>> feature
bottom line
";

    const CONFLICT_DIFF3: &str = "\
<<<<<<< HEAD
our change
||||||| base
original
=======
their change
>>>>>>> feature
";

    #[test]
    fn parses_context_and_region() {
        let p = parse_conflicts(CONFLICT);
        assert!(!p.has_base);
        assert_eq!(p.segments.len(), 3, "context, conflict, context");
        match &p.segments[1] {
            ConflictSegment::Conflict(r) => {
                assert_eq!(r.ours, vec!["our change"]);
                assert_eq!(r.theirs, vec!["their change"]);
                assert!(r.base.is_empty());
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn parses_diff3_base() {
        let p = parse_conflicts(CONFLICT_DIFF3);
        assert!(p.has_base);
        match &p.segments[0] {
            ConflictSegment::Conflict(r) => {
                assert_eq!(r.base, vec!["original"]);
                assert_eq!(r.ours, vec!["our change"]);
                assert_eq!(r.theirs, vec!["their change"]);
            }
            other => panic!("expected conflict first, got {other:?}"),
        }
    }

    #[test]
    fn resolve_takes_chosen_side() {
        let p = parse_conflicts(CONFLICT);
        let ours = resolve(&p, true);
        assert_eq!(ours, "top line\nour change\nbottom line\n");
        let theirs = resolve(&p, false);
        assert_eq!(theirs, "top line\ntheir change\nbottom line\n");
    }

    #[test]
    fn no_markers_is_all_context() {
        let p = parse_conflicts("just\ntext\n");
        assert_eq!(p.segments.len(), 1);
        assert!(matches!(p.segments[0], ConflictSegment::Context(_)));
        assert_eq!(resolve(&p, true), "just\ntext\n");
    }
}
