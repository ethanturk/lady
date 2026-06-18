//! `lady-git` — the engine abstraction for Lady's git reads.
//!
//! [`GitEngine`] is the single trait all read backends implement (gix today,
//! git2/shell later) per ADR-0003. This crate defines the contract only; the
//! gix-backed implementation arrives in US-006.

use lady_proto::{CommitMeta, Oid, RefInfo, RepoId};

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
