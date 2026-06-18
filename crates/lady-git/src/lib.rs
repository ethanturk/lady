//! `lady-git` — the engine abstraction for Lady's git reads.
//!
//! [`GitEngine`] is the single trait all read backends implement (gix today,
//! git2/shell later) per ADR-0003. This crate defines the contract only; the
//! gix-backed implementation arrives in US-006.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use lady_proto::{
    Blame, BlameLine, CommitMeta, FileDiff, FileDiffKind, Oid, RefInfo, RefKind, RepoId, Signature,
};

/// Errors surfaced by a [`GitEngine`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The path is not a git repository, or could not be opened.
    #[error("failed to open repository at {path}: {source}")]
    Open {
        /// The path that was attempted.
        path: String,
        /// The underlying backend error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The referenced repository handle is unknown to this engine.
    #[error("unknown repository: {0:?}")]
    UnknownRepo(RepoId),

    /// A backend operation failed while reading refs or history.
    #[error("git backend error: {0}")]
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

/// Result alias for engine operations.
pub type Result<T> = std::result::Result<T, Error>;

/// A bounded request to walk commit history.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GraphQuery {
    /// Starting commit; `None` means the repository's `HEAD`.
    pub start: Option<Oid>,
    /// Maximum number of commits to return.
    pub limit: usize,
}

/// A read backend over a git repository.
///
/// Implementations open a repo to a [`RepoId`] handle, then serve refs and
/// history reads against that handle. `Send + Sync` so engines can be shared
/// across threads (the UI and core run on separate threads).
pub trait GitEngine: Send + Sync {
    /// Open an existing repository at `path`, returning a handle.
    fn open(&self, path: &std::path::Path) -> Result<RepoId>;

    /// List the repository's refs (branches, tags, remotes, HEAD).
    fn list_refs(&self, repo: &RepoId) -> Result<Vec<RefInfo>>;

    /// Walk history to a flat, ordered list of commits per `query`.
    fn walk_log(&self, repo: &RepoId, query: GraphQuery) -> Result<Vec<CommitMeta>>;

    /// Diff a commit against its first parent (or the empty tree for root commits).
    /// Returns one `FileDiff` per changed file.
    fn diff_commit(&self, repo: &RepoId, commit: &Oid) -> Result<Vec<FileDiff>>;

    /// Annotate each line of `path` with the commit that last changed it.
    /// `at` selects the revision to blame from (`None` = `HEAD`).
    fn blame(&self, repo: &RepoId, path: &str, at: Option<&Oid>) -> Result<Blame>;

    /// List commits (newest first) that changed `path`, walking from `HEAD`.
    /// A commit is included when its blob at `path` differs from its first
    /// parent's (covering add, modify, and delete).
    fn file_history(&self, repo: &RepoId, path: &str) -> Result<Vec<CommitMeta>>;
}

/// A [`GitEngine`] backed by [`gix`] for read-only access (ADR-0003).
///
/// Opened repositories are held in an internal registry keyed by [`RepoId`]
/// (minted from the repo's git-dir path), so later `list_refs`/`walk_log`
/// calls resolve the handle without re-opening. Stored as
/// [`gix::ThreadSafeRepository`] so the engine stays `Send + Sync`.
pub struct GixEngine {
    repos: Mutex<HashMap<RepoId, gix::ThreadSafeRepository>>,
}

/// Wrap any backend error as [`Error::Backend`].
fn backend<E: std::error::Error + Send + Sync + 'static>(e: E) -> Error {
    Error::Backend(Box::new(e))
}

impl GixEngine {
    /// Create an engine with an empty repository registry.
    pub fn new() -> Self {
        GixEngine {
            repos: Mutex::new(HashMap::new()),
        }
    }

    /// Resolve a [`RepoId`] handle to a thread-local [`gix::Repository`].
    ///
    /// Errors with [`Error::UnknownRepo`] if the handle was never `open`ed by
    /// this engine. The stored [`gix::ThreadSafeRepository`] is cloned into a
    /// per-call thread-local repo (cheap; shares the underlying object store).
    fn repo(&self, id: &RepoId) -> Result<gix::Repository> {
        let guard = self
            .repos
            .lock()
            .expect("GixEngine repo registry mutex poisoned");
        let shared = guard
            .get(id)
            .ok_or_else(|| Error::UnknownRepo(id.clone()))?;
        Ok(shared.to_thread_local())
    }
}

impl Default for GixEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GitEngine for GixEngine {
    fn open(&self, path: &Path) -> Result<RepoId> {
        // `gix::open` is non-discovering: it opens exactly `path` and errors on
        // a non-repo directory (the behavior the empty-tempdir test relies on).
        let repo = gix::open(path).map_err(|source| Error::Open {
            path: path.display().to_string(),
            source: Box::new(source),
        })?;
        let id = RepoId::from(repo.git_dir().display().to_string());
        self.repos
            .lock()
            .expect("GixEngine repo registry mutex poisoned")
            .insert(id.clone(), repo.into_sync());
        Ok(id)
    }

    fn list_refs(&self, repo: &RepoId) -> Result<Vec<RefInfo>> {
        let repo = self.repo(repo)?;
        let platform = repo.references().map_err(backend)?;

        let mut out = Vec::new();

        // Local branches, tags, and remote-tracking refs. Each ref is fully
        // peeled to its target object (annotated tags peel through to the
        // commit they point at).
        let groups = [
            (RefKind::Branch, platform.local_branches().map_err(backend)?),
            (RefKind::Tag, platform.tags().map_err(backend)?),
            (
                RefKind::Remote,
                platform.remote_branches().map_err(backend)?,
            ),
        ];
        for (kind, iter) in groups {
            for reference in iter {
                // The iterator already yields a boxed `dyn Error`, matching the
                // `Error::Backend` payload exactly — wrap it directly.
                let mut reference = reference.map_err(Error::Backend)?;
                let name = reference.name().shorten().to_string();
                let target = reference.peel_to_id().map_err(backend)?;
                out.push(RefInfo {
                    name,
                    kind,
                    target: Oid::from(target.detach().to_string()),
                });
            }
        }

        // HEAD (detached-aware): include it when it resolves to a commit. An
        // unborn HEAD (fresh repo, no commits) is simply omitted.
        if let Ok(head) = repo.head_id() {
            out.push(RefInfo {
                name: "HEAD".to_string(),
                kind: RefKind::Head,
                target: Oid::from(head.detach().to_string()),
            });
        }

        Ok(out)
    }

    fn walk_log(&self, repo: &RepoId, query: GraphQuery) -> Result<Vec<CommitMeta>> {
        let repo = self.repo(repo)?;

        // Start point: an explicit oid, else the resolved HEAD tip. An unborn
        // HEAD (empty repo) surfaces as a clean backend error.
        let start = match &query.start {
            Some(oid) => gix::ObjectId::from_hex(oid.as_str().as_bytes()).map_err(backend)?,
            None => repo.head_id().map_err(backend)?.detach(),
        };

        // `limit == 0` means "no cap" (the `GraphQuery::default` case); any
        // positive value caps the number of commits returned.
        let cap = if query.limit == 0 {
            usize::MAX
        } else {
            query.limit
        };

        let mut out = Vec::new();
        for info in repo.rev_walk([start]).all().map_err(backend)? {
            if out.len() >= cap {
                break;
            }
            let info = info.map_err(backend)?;
            let commit = info.object().map_err(backend)?;
            out.push(commit_meta(&commit)?);
        }
        Ok(out)
    }

    fn diff_commit(&self, repo: &RepoId, commit_oid: &Oid) -> Result<Vec<FileDiff>> {
        use std::collections::HashMap;
        let repo = self.repo(repo)?;

        let commit = repo
            .find_commit(gix::ObjectId::from_hex(commit_oid.as_str().as_bytes()).map_err(backend)?)
            .map_err(backend)?;

        let new_tree_id = commit.tree().map_err(backend)?.id;

        // Parent tree (empty tree ObjectId for root commits).
        let old_tree_id: Option<gix::ObjectId> = commit
            .parent_ids()
            .next()
            .map(|pid| {
                repo.find_commit(pid.detach())
                    .map_err(backend)
                    .and_then(|p| Ok(p.tree().map_err(backend)?.id))
            })
            .transpose()?;

        // Collect (path → blob_id) for both trees.
        let mut old_blobs: HashMap<String, gix::ObjectId> = HashMap::new();
        let mut new_blobs: HashMap<String, gix::ObjectId> = HashMap::new();

        if let Some(old_id) = old_tree_id {
            collect_tree_blobs(&repo, old_id, String::new(), &mut old_blobs)?;
        }
        collect_tree_blobs(&repo, new_tree_id, String::new(), &mut new_blobs)?;

        // Diff the two sets.
        let mut diffs: Vec<FileDiff> = Vec::new();

        // Added files (in new but not old).
        for (path, new_id) in &new_blobs {
            if !old_blobs.contains_key(path) {
                let bd = blob_diff(&repo, None, Some(*new_id), path)?;
                diffs.push(FileDiff {
                    path: path.clone(),
                    old_path: None,
                    kind: bd.kind,
                    hunks: bd.hunks,
                    old_image_b64: bd.old_image_b64,
                    new_image_b64: bd.new_image_b64,
                });
            }
        }

        // Deleted files (in old but not new).
        for (path, old_id) in &old_blobs {
            if !new_blobs.contains_key(path) {
                let bd = blob_diff(&repo, Some(*old_id), None, path)?;
                diffs.push(FileDiff {
                    path: path.clone(),
                    old_path: None,
                    kind: bd.kind,
                    hunks: bd.hunks,
                    old_image_b64: bd.old_image_b64,
                    new_image_b64: bd.new_image_b64,
                });
            }
        }

        // Modified files (in both, different OID).
        for (path, new_id) in &new_blobs {
            if let Some(old_id) = old_blobs.get(path) {
                if old_id != new_id {
                    let bd = blob_diff(&repo, Some(*old_id), Some(*new_id), path)?;
                    diffs.push(FileDiff {
                        path: path.clone(),
                        old_path: None,
                        kind: bd.kind,
                        hunks: bd.hunks,
                        old_image_b64: bd.old_image_b64,
                        new_image_b64: bd.new_image_b64,
                    });
                }
            }
        }

        diffs.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(diffs)
    }

    fn blame(&self, repo: &RepoId, path: &str, at: Option<&Oid>) -> Result<Blame> {
        use gix::bstr::BStr;
        use std::collections::HashMap;

        let repo = self.repo(repo)?;

        // Suspect revision: explicit oid, else the resolved HEAD tip.
        let suspect = match at {
            Some(oid) => gix::ObjectId::from_hex(oid.as_str().as_bytes()).map_err(backend)?,
            None => repo.head_id().map_err(backend)?.detach(),
        };

        let outcome = repo
            .blame_file(
                BStr::new(path.as_bytes()),
                suspect,
                gix::repository::blame_file::Options::default(),
            )
            .map_err(backend)?;

        // The blob is the full file content; index its lines for content text.
        let blob = String::from_utf8_lossy(&outcome.blob);
        let file_lines: Vec<&str> = blob.lines().collect();

        // Cache commit (author, time) lookups so each source commit resolves once.
        let mut commit_info: HashMap<gix::ObjectId, (String, i64)> = HashMap::new();
        let mut resolve = |id: gix::ObjectId| -> Result<(String, i64)> {
            if let Some(v) = commit_info.get(&id) {
                return Ok(v.clone());
            }
            let commit = repo.find_commit(id).map_err(backend)?;
            let author = commit.author().map_err(backend)?.name.to_string();
            let time = commit.time().map_err(backend)?.seconds;
            let v = (author, time);
            commit_info.insert(id, v.clone());
            Ok(v)
        };

        // Expand each hunk (a contiguous run) into per-line annotations.
        let mut entries = outcome.entries.clone();
        entries.sort_by_key(|e| e.start_in_blamed_file);

        let mut lines = Vec::new();
        for entry in entries {
            let (author, time) = resolve(entry.commit_id)?;
            let commit = Oid::from(entry.commit_id.to_string());
            let start = entry.start_in_blamed_file;
            for offset in 0..entry.len.get() {
                let idx = (start + offset) as usize;
                let content = file_lines.get(idx).copied().unwrap_or("").to_owned();
                lines.push(BlameLine {
                    line_no: start + offset + 1,
                    commit: commit.clone(),
                    author: author.clone(),
                    time,
                    content,
                });
            }
        }

        Ok(Blame {
            path: path.to_owned(),
            lines,
        })
    }

    fn file_history(&self, repo: &RepoId, path: &str) -> Result<Vec<CommitMeta>> {
        let repo = self.repo(repo)?;
        let head = repo.head_id().map_err(backend)?.detach();
        let rel = std::path::Path::new(path);

        // Blob id of `path` within a commit's tree (None if the path is absent).
        let blob_at = |commit: &gix::Commit| -> Result<Option<gix::ObjectId>> {
            let tree = commit.tree().map_err(backend)?;
            Ok(tree
                .lookup_entry_by_path(rel)
                .map_err(backend)?
                .map(|e| e.object_id()))
        };

        let mut out = Vec::new();
        for info in repo.rev_walk([head]).all().map_err(backend)? {
            let info = info.map_err(backend)?;
            let commit = info.object().map_err(backend)?;
            let current = blob_at(&commit)?;

            // Compare against the first parent (empty tree for a root commit).
            let parent = match commit.parent_ids().next() {
                Some(pid) => {
                    let p = repo.find_commit(pid.detach()).map_err(backend)?;
                    blob_at(&p)?
                }
                None => None,
            };

            if current != parent {
                out.push(commit_meta(&commit)?);
            }
        }
        Ok(out)
    }
}

// ── Tree helpers ──────────────────────────────────────────────────────────────

/// Recursively collect all blob OIDs in a tree into `out`, keyed by path.
fn collect_tree_blobs(
    repo: &gix::Repository,
    tree_id: gix::ObjectId,
    prefix: String,
    out: &mut std::collections::HashMap<String, gix::ObjectId>,
) -> Result<()> {
    let tree = repo.find_tree(tree_id).map_err(backend)?;
    for entry_result in tree.iter() {
        let entry = entry_result.map_err(backend)?;
        let name = entry.inner.filename.to_string();
        let full_path = if prefix.is_empty() {
            name
        } else {
            format!("{}/{}", prefix, name)
        };
        let mode = entry.inner.mode;
        let oid = entry.inner.oid.to_owned();
        if mode.is_tree() {
            collect_tree_blobs(repo, oid, full_path, out)?;
        } else if mode.is_blob() || mode.is_blob_or_symlink() {
            out.insert(full_path, oid);
        }
    }
    Ok(())
}

/// Outcome of diffing one pair of blob OIDs: the change kind, text hunks
/// (empty for binary/image), and base64 image bytes (image diffs only).
struct BlobDiff {
    kind: FileDiffKind,
    hunks: Vec<lady_proto::DiffHunk>,
    old_image_b64: Option<String>,
    new_image_b64: Option<String>,
}

/// Determine `FileDiffKind` and compute text hunks (or image b64) for a
/// pair of optional blob OIDs.
fn blob_diff(
    repo: &gix::Repository,
    old_id: Option<gix::ObjectId>,
    new_id: Option<gix::ObjectId>,
    path: &str,
) -> Result<BlobDiff> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use lady_diff::text_diff;

    // Detect image by extension.
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "tiff" | "webp" | "svg"
    ) {
        let kind = match (old_id, new_id) {
            (None, _) => FileDiffKind::Added,
            (_, None) => FileDiffKind::Deleted,
            _ => FileDiffKind::Image,
        };
        let old_b64 = old_id
            .map(|id| {
                repo.find_object(id)
                    .map_err(backend)
                    .map(|o| B64.encode(&o.data))
            })
            .transpose()?;
        let new_b64 = new_id
            .map(|id| {
                repo.find_object(id)
                    .map_err(backend)
                    .map(|o| B64.encode(&o.data))
            })
            .transpose()?;
        return Ok(BlobDiff {
            kind,
            hunks: Vec::new(),
            old_image_b64: old_b64,
            new_image_b64: new_b64,
        });
    }

    let old_bytes: Vec<u8> = match old_id {
        Some(id) => repo.find_object(id).map_err(backend)?.data.to_vec(),
        None => Vec::new(),
    };
    let new_bytes: Vec<u8> = match new_id {
        Some(id) => repo.find_object(id).map_err(backend)?.data.to_vec(),
        None => Vec::new(),
    };

    // Binary detection: look for null bytes.
    let is_binary = old_bytes.contains(&0) || new_bytes.contains(&0);
    if is_binary {
        let kind = match (old_id, new_id) {
            (None, _) => FileDiffKind::Added,
            (_, None) => FileDiffKind::Deleted,
            _ => FileDiffKind::Binary,
        };
        return Ok(BlobDiff {
            kind,
            hunks: Vec::new(),
            old_image_b64: None,
            new_image_b64: None,
        });
    }

    let old_text = String::from_utf8_lossy(&old_bytes);
    let new_text = String::from_utf8_lossy(&new_bytes);

    let kind = match (old_id, new_id) {
        (None, _) => FileDiffKind::Added,
        (_, None) => FileDiffKind::Deleted,
        _ => FileDiffKind::Modified,
    };
    let hunks = text_diff(&old_text, &new_text);
    Ok(BlobDiff {
        kind,
        hunks,
        old_image_b64: None,
        new_image_b64: None,
    })
}

/// Convert a [`gix::Commit`] into the GUI-agnostic [`CommitMeta`] contract.
fn commit_meta(commit: &gix::Commit) -> Result<CommitMeta> {
    let oid = Oid::from(commit.id().detach().to_string());
    let parents = commit
        .parent_ids()
        .map(|id| Oid::from(id.detach().to_string()))
        .collect();
    let author = signature(commit.author().map_err(backend)?);
    let committer = signature(commit.committer().map_err(backend)?);
    let summary = commit.message().map_err(backend)?.summary().to_string();
    // Committer time, Unix seconds.
    let time = commit.time().map_err(backend)?.seconds;
    Ok(CommitMeta {
        oid,
        parents,
        author,
        committer,
        summary,
        time,
    })
}

/// Map a borrowed gix signature into the owned [`Signature`] contract.
fn signature(sig: gix::actor::SignatureRef<'_>) -> Signature {
    Signature {
        name: sig.name.to_string(),
        email: sig.email.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    /// Run `git` in `dir`, asserting success. System git is permitted for
    /// test-fixture setup only (ADR-0003).
    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .status()
            .expect("system git must be installed to run lady-git tests");
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    }

    /// Build a throwaway repo (tempdir) with three commits on `main`.
    ///
    /// Reusable by US-007 (`list_refs`) and US-008 (`walk_log`). Local config
    /// is set explicitly (no reliance on host global config) and commit signing
    /// is disabled so the fixture is deterministic on any developer/CI machine.
    pub(super) fn fixture_repo() -> TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);
        for i in 1..=3 {
            std::fs::write(p.join(format!("file{i}.txt")), format!("content {i}\n"))
                .expect("write fixture file");
            git(p, &["add", "."]);
            git(p, &["commit", "-q", "-m", &format!("commit {i}")]);
        }
        // A lightweight tag on the tip, so `list_refs` (US-007) exercises the
        // tag path and `walk_log` (US-008) has a non-branch start point.
        git(p, &["tag", "v1"]);
        dir
    }

    #[test]
    fn open_succeeds_on_fixture() {
        let dir = fixture_repo();
        let engine = GixEngine::new();
        let id = engine.open(dir.path()).expect("open the fixture repo");
        assert!(!id.as_str().is_empty(), "RepoId handle should be non-empty");
    }

    #[test]
    fn list_refs_covers_branch_tag_and_head() {
        let dir = fixture_repo();
        let engine = GixEngine::new();
        let id = engine.open(dir.path()).expect("open the fixture repo");
        let refs = engine.list_refs(&id).expect("list_refs on the fixture");

        let named = |kind: RefKind, name: &str| {
            refs.iter()
                .find(|r| r.kind == kind && r.name == name)
                .unwrap_or_else(|| panic!("expected {kind:?} {name:?} in {refs:?}"))
        };

        let branch = named(RefKind::Branch, "main");
        let tag = named(RefKind::Tag, "v1");
        let head = named(RefKind::Head, "HEAD");

        // HEAD resolves to the same commit as `main` (and `v1`, the tip tag).
        assert_eq!(head.target, branch.target, "HEAD should resolve to main");
        assert_eq!(tag.target, branch.target, "v1 tags the tip of main");
        assert!(!head.target.as_str().is_empty(), "HEAD must resolve");

        // Exactly one local branch and no remote-tracking refs in the fixture.
        assert_eq!(
            refs.iter().filter(|r| r.kind == RefKind::Branch).count(),
            1,
            "fixture has only `main`"
        );
        assert_eq!(
            refs.iter().filter(|r| r.kind == RefKind::Remote).count(),
            0,
            "fixture has no remotes"
        );
    }

    #[test]
    fn walk_log_returns_commits_newest_first_and_honors_limit() {
        let dir = fixture_repo();
        let engine = GixEngine::new();
        let id = engine.open(dir.path()).expect("open the fixture repo");

        // Default start (HEAD), no cap: all three commits, newest first.
        let all = engine
            .walk_log(&id, GraphQuery::default())
            .expect("walk_log on the fixture");
        assert_eq!(all.len(), 3, "fixture has three commits");
        let summaries: Vec<&str> = all.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries, ["commit 3", "commit 2", "commit 1"]);

        // The root commit has no parents; later commits have exactly one.
        assert_eq!(all[2].parents.len(), 0, "commit 1 is the root");
        assert_eq!(all[0].parents.len(), 1, "commit 3 has one parent");
        assert_eq!(all[0].parents[0], all[1].oid, "parent links to commit 2");

        // Signatures and time are populated from the fixture config.
        assert_eq!(all[0].author.name, "Lady Test");
        assert_eq!(all[0].committer.email, "test@example.com");
        assert!(all[0].time > 0, "committer time should be set");

        // `limit` caps the result.
        let two = engine
            .walk_log(
                &id,
                GraphQuery {
                    start: None,
                    limit: 2,
                },
            )
            .expect("walk_log with a limit");
        assert_eq!(two.len(), 2, "limit of 2 returns two commits");
        assert_eq!(two[0].summary, "commit 3");
    }

    #[test]
    fn walk_log_errors_on_unknown_repo() {
        let engine = GixEngine::new();
        let err = engine
            .walk_log(
                &RepoId::from("never-opened".to_string()),
                GraphQuery::default(),
            )
            .expect_err("walk_log on an unopened handle must fail");
        assert!(
            matches!(err, Error::UnknownRepo(_)),
            "expected Error::UnknownRepo, got {err:?}"
        );
    }

    #[test]
    fn list_refs_errors_on_unknown_repo() {
        let engine = GixEngine::new();
        let err = engine
            .list_refs(&RepoId::from("never-opened".to_string()))
            .expect_err("list_refs on an unopened handle must fail");
        assert!(
            matches!(err, Error::UnknownRepo(_)),
            "expected Error::UnknownRepo, got {err:?}"
        );
    }

    #[test]
    fn open_errors_on_empty_dir() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let engine = GixEngine::new();
        let err = engine
            .open(dir.path())
            .expect_err("opening a non-repo dir must fail");
        assert!(
            matches!(err, Error::Open { .. }),
            "expected Error::Open, got {err:?}"
        );
    }

    #[test]
    fn blame_attributes_lines_to_introducing_commits() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        // Commit 1 introduces two lines.
        std::fs::write(p.join("a.txt"), "line1\nline2\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "first"]);

        // Commit 2 changes line 2 and appends line 3.
        std::fs::write(p.join("a.txt"), "line1\nCHANGED\nline3\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "second"]);

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");
        let blame = engine.blame(&id, "a.txt", None).expect("blame a.txt");

        assert_eq!(blame.path, "a.txt");
        assert_eq!(blame.lines.len(), 3, "three lines blamed");

        // Lines are in file order with 1-indexed numbers.
        assert_eq!(blame.lines[0].line_no, 1);
        assert_eq!(blame.lines[0].content, "line1");
        assert_eq!(blame.lines[1].content, "CHANGED");
        assert_eq!(blame.lines[2].content, "line3");

        // Line 1 came from the first commit; lines 2 and 3 from the second.
        assert_ne!(
            blame.lines[0].commit, blame.lines[1].commit,
            "line 1 and the changed line 2 have different source commits"
        );
        assert_eq!(
            blame.lines[1].commit, blame.lines[2].commit,
            "the changed line and appended line share the second commit"
        );
        assert_eq!(blame.lines[0].author, "Lady Test");
        assert!(blame.lines[0].time > 0, "commit time populated");
    }

    #[test]
    fn file_history_lists_only_commits_touching_the_path() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        // c1: add a.txt + b.txt.
        std::fs::write(p.join("a.txt"), "a1\n").expect("write");
        std::fs::write(p.join("b.txt"), "b1\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "add a and b"]);

        // c2: modify a.txt only.
        std::fs::write(p.join("a.txt"), "a2\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "edit a"]);

        // c3: modify b.txt only (must NOT appear in a.txt history).
        std::fs::write(p.join("b.txt"), "b2\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "edit b"]);

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");
        let hist = engine
            .file_history(&id, "a.txt")
            .expect("file_history a.txt");

        let summaries: Vec<&str> = hist.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(
            summaries,
            vec!["edit a", "add a and b"],
            "newest-first, only commits touching a.txt"
        );
    }
}
