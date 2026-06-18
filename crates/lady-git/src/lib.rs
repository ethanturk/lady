//! `lady-git` — the engine abstraction for Lady's git reads.
//!
//! [`GitEngine`] is the single trait all read backends implement (gix today,
//! git2/shell later) per ADR-0003. This crate defines the contract only; the
//! gix-backed implementation arrives in US-006.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use lady_proto::{CommitMeta, Oid, RefInfo, RefKind, RepoId, Signature};

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
}
