//! `lady-proto` — shared, GUI-agnostic types forming the contract between the
//! Lady core engine and any frontend. This crate has zero git-library deps.
//!
//! These types are the serializable wire contract (PLAN.md §2.3, §4.1). They
//! intentionally carry no behavior beyond serde round-tripping so that the
//! engine, the (future) UI, and any IPC layer all agree on one shape.

use serde::{Deserialize, Serialize};

/// A git object id, kept as its hex string so this crate stays free of any
/// git-library dependency. Engines convert to/from their native id type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Oid(pub String);

impl Oid {
    /// Borrow the underlying hex string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Oid {
    fn from(s: String) -> Self {
        Oid(s)
    }
}

impl From<&str> for Oid {
    fn from(s: &str) -> Self {
        Oid(s.to_owned())
    }
}

/// Opaque handle to an opened repository, minted by the engine on `open()`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoId(pub String);

impl RepoId {
    /// Borrow the underlying handle string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RepoId {
    fn from(s: String) -> Self {
        RepoId(s)
    }
}

/// The category of a git reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RefKind {
    /// A local branch (e.g. `refs/heads/main`).
    Branch,
    /// A tag (e.g. `refs/tags/v1.0`).
    Tag,
    /// A remote-tracking ref (e.g. `refs/remotes/origin/main`).
    Remote,
    /// The repository `HEAD`.
    Head,
}

/// A single git reference resolved to its target object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefInfo {
    /// The ref's name (e.g. `main`, `v1.0`, `origin/main`, `HEAD`).
    pub name: String,
    /// What kind of ref this is.
    pub kind: RefKind,
    /// The object the ref points at.
    pub target: Oid,
}

/// A commit author or committer signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
}

/// Metadata for a single commit, as surfaced by a log walk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitMeta {
    /// This commit's object id.
    pub oid: Oid,
    /// Parent commit ids (0 for a root, 1 for a normal commit, 2+ for merges).
    pub parents: Vec<Oid>,
    /// Who wrote the change.
    pub author: Signature,
    /// Who committed it.
    pub committer: Signature,
    /// First line of the commit message.
    pub summary: String,
    /// Commit time as Unix seconds (committer time).
    pub time: i64,
}

// ── Diff types ────────────────────────────────────────────────────────────────

/// Whether a diff line was added, deleted, or unchanged (context).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineKind {
    Added,
    Deleted,
    Context,
}

/// A single line in a diff hunk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Raw line content (without the leading +/- prefix).
    pub content: String,
}

/// A contiguous block of changed lines with surrounding context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Start line (1-indexed) in the old file.
    pub old_start: u32,
    /// Number of old-file lines this hunk spans.
    pub old_lines: u32,
    /// Start line (1-indexed) in the new file.
    pub new_start: u32,
    /// Number of new-file lines this hunk spans.
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// High-level change category for a file entry in a diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileDiffKind {
    Added,
    Deleted,
    Modified,
    /// File content is binary — no text hunks available.
    Binary,
    /// File is an image — frontend should display both versions visually.
    Image,
}

/// Diff for a single file between two commits (or working tree vs HEAD).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    /// Relative file path (forward slashes).
    pub path: String,
    /// Old path if the file was renamed; `None` otherwise.
    pub old_path: Option<String>,
    pub kind: FileDiffKind,
    /// Text hunks; empty for binary and image diffs.
    pub hunks: Vec<DiffHunk>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_commit() -> CommitMeta {
        CommitMeta {
            oid: Oid::from("a".repeat(40)),
            parents: vec![Oid::from("b".repeat(40)), Oid::from("c".repeat(40))],
            author: Signature {
                name: "Ada Lovelace".to_owned(),
                email: "ada@example.com".to_owned(),
            },
            committer: Signature {
                name: "Grace Hopper".to_owned(),
                email: "grace@example.com".to_owned(),
            },
            summary: "Introduce the analytical engine".to_owned(),
            time: 1_700_000_000,
        }
    }

    #[test]
    fn commit_meta_serde_round_trip() {
        let commit = sample_commit();
        let json = serde_json::to_string(&commit).expect("serialize CommitMeta");
        let back: CommitMeta = serde_json::from_str(&json).expect("deserialize CommitMeta");
        assert_eq!(commit, back);
    }

    #[test]
    fn ref_info_serde_round_trip() {
        for kind in [
            RefKind::Branch,
            RefKind::Tag,
            RefKind::Remote,
            RefKind::Head,
        ] {
            let info = RefInfo {
                name: "main".to_owned(),
                kind,
                target: Oid::from("d".repeat(40)),
            };
            let json = serde_json::to_string(&info).expect("serialize RefInfo");
            let back: RefInfo = serde_json::from_str(&json).expect("deserialize RefInfo");
            assert_eq!(info, back);
        }
    }
}
