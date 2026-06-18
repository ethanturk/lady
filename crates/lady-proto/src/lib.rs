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
    /// Base64-encoded old-version image bytes (image diffs only).
    pub old_image_b64: Option<String>,
    /// Base64-encoded new-version image bytes (image diffs only).
    pub new_image_b64: Option<String>,
}

// ── Blame types ───────────────────────────────────────────────────────────────

/// One line of a file annotated with the commit that last changed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    /// 1-indexed line number in the blamed file.
    pub line_no: u32,
    /// The commit that introduced this line.
    pub commit: Oid,
    /// Author name of that commit.
    pub author: String,
    /// Committer time of that commit (Unix seconds).
    pub time: i64,
    /// The line's text content (no trailing newline).
    pub content: String,
}

/// Per-line blame for a single file at a given revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blame {
    /// Relative file path (forward slashes).
    pub path: String,
    /// Lines in file order.
    pub lines: Vec<BlameLine>,
}

// ── Working-tree status types ───────────────────────────────────────────────────

/// How a path changed relative to its baseline (index or HEAD).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    /// A new file.
    Added,
    /// Content changed.
    Modified,
    /// The file was removed.
    Deleted,
    /// The file was renamed (see [`FileStatus::old_path`]).
    Renamed,
    /// An untracked file (present on disk, unknown to git).
    Untracked,
    /// The file is in a merge conflict.
    Conflicted,
}

/// One changed path in the working tree, in either the staged or unstaged set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatus {
    /// Relative path (forward slashes).
    pub path: String,
    /// Previous path for a rename (`None` otherwise).
    pub old_path: Option<String>,
    /// What kind of change this is.
    pub kind: ChangeKind,
}

/// A snapshot of the working tree split into staged, unstaged, and untracked.
///
/// Mirrors `git status --porcelain=v2`: a single path can appear in both
/// `staged` and `unstaged` (e.g. a staged change with further on-disk edits).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingTree {
    /// Changes staged in the index (X column of the status code).
    pub staged: Vec<FileStatus>,
    /// Tracked changes not yet staged (Y column of the status code).
    pub unstaged: Vec<FileStatus>,
    /// Untracked paths (forward slashes).
    pub untracked: Vec<String>,
}

/// How far the current branch sits from its upstream: `ahead` commits exist
/// locally but not upstream, `behind` exist upstream but not locally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AheadBehind {
    pub ahead: usize,
    pub behind: usize,
}

/// One entry in the stash stack (`git stash list`). `index` is the position in
/// the reflog (`stash@{index}`); 0 is the most recent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    /// Position in the stash stack (`stash@{index}`).
    pub index: usize,
    /// The stash's subject line (e.g. `On main: WIP`).
    pub message: String,
    /// The stash commit's object id.
    pub oid: Oid,
}

/// Fast-forward policy for a merge (mirrors git's `--ff` / `--ff-only` /
/// `--no-ff`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FfMode {
    /// Fast-forward when possible, otherwise create a merge commit (git default).
    #[default]
    Auto,
    /// Refuse the merge unless it can fast-forward (`--ff-only`).
    Only,
    /// Always create a merge commit, even when a fast-forward is possible
    /// (`--no-ff`).
    Never,
}

/// Result of a merge attempt.
///
/// Conflict *resolution* (a 3-pane editor) is Phase 3; here a conflicted merge
/// only reports the conflicted paths so the UI can list them and offer abort.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum MergeOutcome {
    /// Target already reachable from HEAD; nothing to do.
    AlreadyUpToDate,
    /// HEAD advanced to the target with no merge commit.
    FastForwarded,
    /// A merge commit was created; carries its object id.
    Merged(Oid),
    /// Merge stopped with conflicts; carries the conflicted paths.
    Conflicts(Vec<String>),
}

/// Result of a sequencer operation (cherry-pick or revert).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ApplyOutcome {
    /// The operation completed and `HEAD` now points at this commit.
    Applied(Oid),
    /// Operation stopped with conflicts; carries the conflicted paths.
    Conflicts(Vec<String>),
}

/// Result of a rebase attempt (plain or interactive).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum RebaseOutcome {
    /// The rebase completed.
    Rebased,
    /// Rebase stopped with conflicts; carries the conflicted paths.
    Conflicts(Vec<String>),
    /// Rebase stopped mid-sequence for an `edit` step (no conflict); the repo
    /// is left mid-rebase awaiting `continue` / `abort`.
    Stopped,
}

/// What to do with a commit in an interactive rebase (mirrors git's todo verbs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseAction {
    /// Keep the commit as-is.
    Pick,
    /// Keep the commit but edit its message.
    Reword,
    /// Stop after applying to amend the commit.
    Edit,
    /// Meld into the previous commit, combining messages.
    Squash,
    /// Meld into the previous commit, discarding this message.
    Fixup,
    /// Remove the commit.
    Drop,
}

impl RebaseAction {
    /// The git rebase-todo keyword for this action.
    pub fn keyword(self) -> &'static str {
        match self {
            RebaseAction::Pick => "pick",
            RebaseAction::Reword => "reword",
            RebaseAction::Edit => "edit",
            RebaseAction::Squash => "squash",
            RebaseAction::Fixup => "fixup",
            RebaseAction::Drop => "drop",
        }
    }
}

/// One step of an interactive-rebase plan. The plan is an ordered `Vec`; the
/// vec order encodes reordering of commits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebaseStep {
    /// The commit this step acts on.
    pub oid: Oid,
    /// What to do with it.
    pub action: RebaseAction,
    /// Replacement message for `Reword` (or combined message for `Squash`).
    pub message: Option<String>,
}

/// Verification status of a commit's signature (from git's `%G?`), surfaced as
/// a badge. Maps git's many codes down to four user-facing states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureStatus {
    /// A valid signature from a trusted key (`G`).
    Good,
    /// A bad / invalid signature (`B`).
    Bad,
    /// A signature that verifies but whose key is untrusted, expired, or
    /// revoked (`U` / `X` / `Y` / `R`).
    Untrusted,
    /// No signature, or git cannot check it (`N` / `E`).
    None,
}

impl SignatureStatus {
    /// Map git's `%G?` code character to a status.
    pub fn from_code(code: &str) -> SignatureStatus {
        match code.trim() {
            "G" => SignatureStatus::Good,
            "B" => SignatureStatus::Bad,
            "U" | "X" | "Y" | "R" => SignatureStatus::Untrusted,
            _ => SignatureStatus::None,
        }
    }
}

/// The input type of a custom-command placeholder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaceholderKind {
    /// Free-text input.
    Text,
    /// A branch chosen from the repo's branches.
    Branch,
    /// A file path chosen from the repo.
    File,
}

/// A typed placeholder parsed from a custom-command template (`{name:kind}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placeholder {
    /// The placeholder's name (the key the runner prompts for).
    pub name: String,
    /// What kind of input it expects.
    pub kind: PlaceholderKind,
}

/// A user-defined custom command (PH3-009), persisted in settings. The template
/// is an argv-style command with `{name:kind}` placeholders.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomCommand {
    /// Display name.
    pub name: String,
    /// Command template with typed placeholders.
    pub template: String,
}

/// Captured result of running a custom command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutput {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Process exit code (`-1` when terminated by a signal).
    pub exit_code: i32,
}

/// One Git LFS-tracked file (`git lfs ls-files`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LfsFile {
    /// Path relative to the repo root.
    pub path: String,
    /// The LFS object's (short) oid.
    pub oid: String,
    /// Whether the real bytes are materialized locally (vs a pointer).
    pub downloaded: bool,
}

/// Git LFS status for a repository (PH4-007).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LfsStatus {
    /// Whether `git-lfs` is installed and usable.
    pub available: bool,
    /// Tracked path patterns (from `.gitattributes`).
    pub patterns: Vec<String>,
    /// LFS-tracked files and whether each is materialized.
    pub files: Vec<LfsFile>,
}

/// Snapshot of an in-progress `git bisect`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BisectState {
    /// The commit currently checked out for testing (`HEAD`), if bisecting.
    pub current_oid: Option<Oid>,
    /// Git's rough estimate of remaining steps.
    pub remaining_steps_estimate: usize,
    /// The identified first-bad commit, once bisect converges.
    pub suspected: Option<Oid>,
}

/// One entry in a ref's reflog (`git reflog`). Powers recovering lost commits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflogEntry {
    /// The commit the ref pointed at after this entry (the "new" oid).
    pub oid: Oid,
    /// The commit the ref pointed at before (the previous entry's oid); empty
    /// for the oldest entry.
    pub prev_oid: Oid,
    /// The action that created the entry (e.g. `commit`, `reset`, `checkout`).
    pub action: String,
    /// The remainder of the reflog subject after the action.
    pub message: String,
    /// Entry time as Unix seconds.
    pub time: i64,
}

/// A linked worktree of a repository (`git worktree list`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    /// Absolute path to the worktree directory.
    pub path: String,
    /// Checked-out branch (short name), or `None` when detached/bare.
    pub branch: Option<String>,
    /// The worktree's `HEAD` commit, or `None` for a bare entry.
    pub head: Option<Oid>,
    /// Whether the worktree is locked.
    pub locked: bool,
}

/// What mid-operation state a repository is in, used to drive conflict
/// resolution and the correct `--abort` path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictState {
    /// No operation in progress.
    None,
    /// Mid-merge (`MERGE_HEAD` present).
    Merge,
    /// Mid-rebase (`rebase-merge`/`rebase-apply` present).
    Rebase,
    /// Mid-cherry-pick (`CHERRY_PICK_HEAD` present).
    CherryPick,
    /// Mid-revert (`REVERT_HEAD` present).
    Revert,
}

/// The three sides of a conflicted file, read from the index stages
/// (`:1:` base, `:2:` ours, `:3:` theirs). A side is `None` when that stage is
/// absent (e.g. an add/add conflict has no base).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictSides {
    /// Common-ancestor content (stage 1), if any.
    pub base: Option<String>,
    /// Our side (stage 2 — current branch / rebase target).
    pub ours: Option<String>,
    /// Their side (stage 3 — incoming).
    pub theirs: Option<String>,
}

/// One conflict hunk parsed from a file's conflict markers. `base` is empty
/// unless the file was written with diff3-style markers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRegion {
    /// Lines between `<<<<<<<` and `|||||||`/`=======` (our side).
    pub ours: Vec<String>,
    /// Lines between `|||||||` and `=======` (the merge base; diff3 only).
    pub base: Vec<String>,
    /// Lines between `=======` and `>>>>>>>` (their side).
    pub theirs: Vec<String>,
}

/// A segment of a parsed conflicted file: either unconflicted context or a
/// conflict region.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ConflictSegment {
    /// A run of unconflicted lines surrounding the conflicts.
    Context(Vec<String>),
    /// A conflict hunk.
    Conflict(ConflictRegion),
}

/// A conflicted file parsed into an ordered sequence of context and conflict
/// segments, preserving the surrounding text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedConflict {
    /// File segments in order.
    pub segments: Vec<ConflictSegment>,
    /// Whether any region carried a diff3 base section.
    pub has_base: bool,
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

    #[test]
    fn blame_serde_round_trip() {
        let blame = Blame {
            path: "src/lib.rs".to_owned(),
            lines: vec![BlameLine {
                line_no: 1,
                commit: Oid::from("a".repeat(40)),
                author: "Ada Lovelace".to_owned(),
                time: 1_700_000_000,
                content: "fn main() {}".to_owned(),
            }],
        };
        let json = serde_json::to_string(&blame).expect("serialize Blame");
        let back: Blame = serde_json::from_str(&json).expect("deserialize Blame");
        assert_eq!(blame, back);
    }

    #[test]
    fn stash_entry_serde_round_trip() {
        let entry = StashEntry {
            index: 2,
            message: "On main: WIP refactor".to_owned(),
            oid: Oid::from("e".repeat(40)),
        };
        let json = serde_json::to_string(&entry).expect("serialize StashEntry");
        let back: StashEntry = serde_json::from_str(&json).expect("deserialize StashEntry");
        assert_eq!(entry, back);
    }

    #[test]
    fn merge_outcome_serde_round_trip() {
        let cases = [
            MergeOutcome::AlreadyUpToDate,
            MergeOutcome::FastForwarded,
            MergeOutcome::Merged(Oid::from("c".repeat(40))),
            MergeOutcome::Conflicts(vec!["a.txt".to_owned(), "b.txt".to_owned()]),
        ];
        for outcome in cases {
            let json = serde_json::to_string(&outcome).expect("serialize MergeOutcome");
            let back: MergeOutcome = serde_json::from_str(&json).expect("deserialize MergeOutcome");
            assert_eq!(outcome, back);
        }
    }

    #[test]
    fn apply_outcome_serde_round_trip() {
        let cases = [
            ApplyOutcome::Applied(Oid::from("a".repeat(40))),
            ApplyOutcome::Conflicts(vec!["conflicted.txt".to_owned()]),
        ];
        for outcome in cases {
            let json = serde_json::to_string(&outcome).expect("serialize ApplyOutcome");
            let back: ApplyOutcome = serde_json::from_str(&json).expect("deserialize ApplyOutcome");
            assert_eq!(outcome, back);
        }
    }

    #[test]
    fn rebase_outcome_serde_round_trip() {
        let cases = [
            RebaseOutcome::Rebased,
            RebaseOutcome::Conflicts(vec!["conflicted.txt".to_owned()]),
            RebaseOutcome::Stopped,
        ];
        for outcome in cases {
            let json = serde_json::to_string(&outcome).expect("serialize RebaseOutcome");
            let back: RebaseOutcome =
                serde_json::from_str(&json).expect("deserialize RebaseOutcome");
            assert_eq!(outcome, back);
        }
    }

    #[test]
    fn working_tree_serde_round_trip() {
        let wt = WorkingTree {
            staged: vec![FileStatus {
                path: "added.rs".to_owned(),
                old_path: None,
                kind: ChangeKind::Added,
            }],
            unstaged: vec![
                FileStatus {
                    path: "edited.rs".to_owned(),
                    old_path: None,
                    kind: ChangeKind::Modified,
                },
                FileStatus {
                    path: "now.rs".to_owned(),
                    old_path: Some("was.rs".to_owned()),
                    kind: ChangeKind::Renamed,
                },
            ],
            untracked: vec!["scratch.tmp".to_owned()],
        };
        let json = serde_json::to_string(&wt).expect("serialize WorkingTree");
        let back: WorkingTree = serde_json::from_str(&json).expect("deserialize WorkingTree");
        assert_eq!(wt, back);
    }
}
