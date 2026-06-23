//! `lady-git` — the engine abstraction for Lady's git reads.
//!
//! [`GitEngine`] is the single trait all read backends implement (gix today,
//! git2/shell later) per ADR-0003. This crate defines the contract only; the
//! gix-backed implementation arrives in US-006.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

pub mod custom;

use lady_proto::{
    AheadBehind, ApplyOutcome, BisectState, Blame, BlameLine, ChangeKind, CommandOutput,
    CommitMeta, ConflictSides, ConflictState, FfMode, FileDiff, FileDiffKind, FileStatus,
    FlowConfig, FlowKind, GitIdentity, LfsFile, LfsStatus, MergeOutcome, Oid, ParsedConflict,
    RebaseOutcome, RebaseStep, RefInfo, RefKind, ReflogEntry, RepoId, RepositoryFamily,
    RepositoryFamilyId, ResetMode, Signature, SignatureStatus, StashEntry, Submodule, WorkingTree,
    Worktree,
};

/// Whether `git-lfs` is installed and usable (`git lfs version`). Free function
/// because availability is independent of any repository (PH4-007).
pub fn lfs_available() -> bool {
    Command::new("git")
        .args(["lfs", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

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

    /// A shell-out git operation failed (non-zero exit, or no worktree). The
    /// payload is git's stderr verbatim where available (ADR-0003 mutation tier).
    #[error("{0}")]
    Git(String),
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

/// What to diff. Generalizes the Phase-1 commit diff to also cover the
/// working-tree and index sides needed by the Changes view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffSpec {
    /// A commit against its first parent (Phase 1 behavior).
    Commit(Oid),
    /// One path: index blob (old) vs the on-disk working file (new) — i.e. the
    /// unstaged changes.
    WorkingVsIndex(String),
    /// One path: HEAD blob (old) vs the index blob (new) — i.e. the staged
    /// changes.
    IndexVsHead(String),
}

/// Options for [`GitEngine::commit`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitOpts {
    /// Replace the tip commit (`git commit --amend`) instead of adding a new
    /// one. The author and date are preserved; the message is replaced.
    pub amend: bool,
    /// Force-sign this commit (`git commit -S`) regardless of `commit.gpgsign`.
    /// When `false`, the user's git config decides (ADR-0006); signing keys,
    /// `gpg.format`, `gpg.program`, and passphrase prompts are git's own.
    pub sign: bool,
}

/// Options for [`GitEngine::merge`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeOpts {
    /// Fast-forward policy (`git merge --ff`, `--ff-only`, or `--no-ff`).
    pub fast_forward: FfMode,
    /// Optional merge commit message. When absent, Lady passes `--no-edit` so
    /// system git never opens an editor during GUI-driven merges.
    pub commit_message: Option<String>,
}

/// Per-invocation authentication overrides for a single network git call.
///
/// `config` pairs are passed as leading `-c <k>=<v>` flags and `env` as process
/// environment, so they apply only to that one child process — never the user's
/// global git config (ADR-0006). The default ([`GitAuth::none`]) leaves the
/// invocation byte-for-byte identical to relying on system git's own helpers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitAuth {
    /// `git -c <key>=<value>` pairs prepended before the subcommand.
    pub config: Vec<(String, String)>,
    /// Environment variables set on the git child (e.g. `GIT_SSH_COMMAND`).
    pub env: Vec<(String, String)>,
}

impl GitAuth {
    /// No overrides — use system git's configured credential helpers / ssh-agent.
    pub fn none() -> Self {
        Self::default()
    }

    /// True when there is nothing to apply (the default path).
    pub fn is_empty(&self) -> bool {
        self.config.is_empty() && self.env.is_empty()
    }
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

    /// Diff the `base` tree against the `head` tree (the net change of the span
    /// `base..head`). Used to plan a recompose without touching the working tree.
    fn diff_range(&self, repo: &RepoId, base: &Oid, head: &Oid) -> Result<Vec<FileDiff>>;

    /// `git reset --soft|--mixed|--hard <target>`. Soft moves HEAD only (changes
    /// staged); mixed also resets the index but keeps the working tree (changes
    /// unstaged); hard discards working-tree changes too (used to roll a failed
    /// recompose back to the original HEAD). See [`ResetMode`].
    fn reset(&self, repo: &RepoId, target: &Oid, mode: ResetMode) -> Result<()>;

    /// The current `HEAD` commit oid.
    fn head_commit(&self, repo: &RepoId) -> Result<Oid>;

    /// Whether `oid` is reachable from the current branch's upstream (i.e. it has
    /// been pushed). `false` when there is no upstream configured.
    fn commit_is_pushed(&self, repo: &RepoId, oid: &Oid) -> Result<bool>;

    /// Diff per a [`DiffSpec`]: a commit, one file's unstaged changes
    /// (working vs index), or one file's staged changes (index vs HEAD).
    fn diff_spec(&self, repo: &RepoId, spec: &DiffSpec) -> Result<Vec<FileDiff>>;

    /// Annotate each line of `path` with the commit that last changed it.
    /// `at` selects the revision to blame from (`None` = `HEAD`).
    fn blame(&self, repo: &RepoId, path: &str, at: Option<&Oid>) -> Result<Blame>;

    /// List commits (newest first) that changed `path`, walking from `HEAD`.
    /// A commit is included when its blob at `path` differs from its first
    /// parent's (covering add, modify, and delete).
    fn file_history(&self, repo: &RepoId, path: &str) -> Result<Vec<CommitMeta>>;

    /// Whether the repository's worktree has uncommitted changes (tracked
    /// modifications or untracked files). Powers the dirty-tab star indicator.
    fn is_dirty(&self, repo: &RepoId) -> Result<bool>;

    /// List every tracked file path at `HEAD` (sorted). Powers the command
    /// palette's file search.
    fn list_files(&self, repo: &RepoId) -> Result<Vec<String>>;

    /// Snapshot the working tree (staged / unstaged / untracked) via shell-out
    /// `git status --porcelain=v2 -z` for exact git semantics (ADR-0003).
    fn status(&self, repo: &RepoId) -> Result<WorkingTree>;

    /// Stage `paths` (whole files) into the index via `git add` — records
    /// adds, modifications, and deletions. A no-op when `paths` is empty.
    fn stage_paths(&self, repo: &RepoId, paths: &[String]) -> Result<()>;

    /// Unstage `paths` (whole files), restoring the index entry to its `HEAD`
    /// state, or removing it from the index on an unborn branch (no commits
    /// yet). A no-op when `paths` is empty.
    fn unstage_paths(&self, repo: &RepoId, paths: &[String]) -> Result<()>;

    /// Apply a unified-diff `patch` via shell-out `git apply`, optionally to the
    /// index (`cached`) and/or reversed (`reverse`). Powers partial staging:
    /// forward+cached stages selected hunks, reverse+cached unstages them, and
    /// reverse (no cached) discards them from the working tree.
    fn apply_patch(&self, repo: &RepoId, patch: &str, reverse: bool, cached: bool) -> Result<()>;

    /// Delete untracked `paths` from the working tree via `git clean -fd`
    /// (DESTRUCTIVE — the caller must confirm first). A no-op when empty.
    fn discard_untracked(&self, repo: &RepoId, paths: &[String]) -> Result<()>;

    /// Commit the staged changes with `message` (`git commit -m`), or rewrite
    /// the tip when `opts.amend` is set (`git commit --amend -m`). Returns the
    /// new commit's [`Oid`]. The user's `commit.gpgsign` config is left intact.
    fn commit(&self, repo: &RepoId, message: &str, opts: &CommitOpts) -> Result<Oid>;

    /// The most recent commit subjects (first lines), newest first, capped at
    /// `limit`. Empty on an unborn branch. Powers message reuse/prefill.
    fn recent_messages(&self, repo: &RepoId, limit: usize) -> Result<Vec<String>>;

    /// Create branch `name` at `start_point` (a branch/revision), or at the
    /// current HEAD when `None`. Fails if the branch already exists.
    fn create_branch(&self, repo: &RepoId, name: &str, start_point: Option<&str>) -> Result<()>;

    /// Delete branch `name`. `force` allows deleting an unmerged branch
    /// (`git branch -D` vs `-d`).
    fn delete_branch(&self, repo: &RepoId, name: &str, force: bool) -> Result<()>;

    /// Rename branch `old` to `new` (`git branch -m`).
    fn rename_branch(&self, repo: &RepoId, old: &str, new: &str) -> Result<()>;

    /// The short upstream of `branch` (e.g. `origin/main`), or `None` when unset.
    fn branch_upstream(&self, repo: &RepoId, branch: &str) -> Result<Option<String>>;

    /// Set (`Some`) or unset (`None`) the upstream tracking ref of `branch`.
    fn set_branch_upstream(
        &self,
        repo: &RepoId,
        branch: &str,
        upstream: Option<&str>,
    ) -> Result<()>;

    /// Fast-forward local `branch` to `upstream` WITHOUT checking it out. Errors
    /// when `branch` is not an ancestor of `upstream` (not fast-forwardable).
    fn fast_forward_branch(&self, repo: &RepoId, branch: &str, upstream: &str) -> Result<()>;

    /// Discard all working-tree + index changes to tracked `paths`
    /// (`git checkout HEAD -- …`). Untracked files use [`Self::discard_untracked`].
    fn discard_files(&self, repo: &RepoId, paths: &[String]) -> Result<()>;

    /// Stash only `paths` (`git stash push [-u] [-m] -- …`).
    fn stash_paths(
        &self,
        repo: &RepoId,
        message: Option<&str>,
        include_untracked: bool,
        paths: &[String],
    ) -> Result<()>;

    /// Write the uncommitted diff of `paths` (`git diff HEAD -- …`) to `dest`.
    fn export_patch(&self, repo: &RepoId, paths: &[String], dest: &str) -> Result<()>;

    /// Append `patterns` (one per line) to the repo-root `.gitignore`.
    fn add_to_gitignore(&self, repo: &RepoId, patterns: &[String]) -> Result<()>;

    /// Resolve `path` (repo-relative) to an absolute path under the workdir.
    fn resolve_path(&self, repo: &RepoId, path: &str) -> Result<PathBuf>;

    /// Check out `target` (a branch name or revision). On a plain revision this
    /// produces a detached HEAD. Refuses (surfacing git's message) when the
    /// switch would overwrite local changes, unless `force` is set.
    fn checkout(&self, repo: &RepoId, target: &str, force: bool) -> Result<()>;

    /// Create tag `name` at `target` (or HEAD when `None`). With a `message` an
    /// annotated tag is created; otherwise a lightweight tag. Fails if it exists.
    fn create_tag(
        &self,
        repo: &RepoId,
        name: &str,
        target: Option<&str>,
        message: Option<&str>,
    ) -> Result<()>;

    /// Delete tag `name`.
    fn delete_tag(&self, repo: &RepoId, name: &str) -> Result<()>;

    /// Move (force-update) an existing tag `name` to `target`. The tag is
    /// recreated as a lightweight tag, even if it was previously annotated.
    fn move_tag(&self, repo: &RepoId, name: &str, target: &str) -> Result<()>;

    /// Fetch from `remote` (or the default remote when `None`), streaming
    /// git's `--progress` output to `on_progress`. Credentials and transport
    /// are supplied by the per-invocation `auth` overrides, or entirely by
    /// system git when `auth` is empty. A failure returns git's message verbatim.
    fn fetch(
        &self,
        repo: &RepoId,
        remote: Option<&str>,
        auth: &GitAuth,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<()>;

    /// Pull (`fetch` + integrate) from `remote`/`branch`, or the configured
    /// upstream when either is `None`. Progress streams to `on_progress`.
    fn pull(
        &self,
        repo: &RepoId,
        remote: Option<&str>,
        branch: Option<&str>,
        auth: &GitAuth,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<()>;

    /// Push the current branch to `remote`/`branch`. When either is omitted and
    /// the branch has no upstream (or `set_upstream` is true), both are inferred
    /// from the current branch and configured remotes (`origin` as fallback).
    /// `set_upstream` records the tracking ref (`-u`); `force` allows a
    /// non-fast-forward update (`--force`). Progress streams to `on_progress`;
    /// rejections surface git's message verbatim.
    #[allow(clippy::too_many_arguments)]
    fn push(
        &self,
        repo: &RepoId,
        remote: Option<&str>,
        branch: Option<&str>,
        set_upstream: bool,
        force: bool,
        auth: &GitAuth,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<()>;

    /// How far the current branch is ahead/behind its upstream, or `None` when
    /// there is no upstream configured (nothing to compare against).
    fn ahead_behind(&self, repo: &RepoId) -> Result<Option<AheadBehind>>;

    /// Ahead/behind counts for branch rows that can be compared to a local /
    /// remote pair. Local branches with an upstream are keyed by local branch
    /// short name; their upstream remote-tracking rows are keyed by remote short
    /// name (for example `origin/main`). Remote-tracking rows with a same-named
    /// local branch are also included even when tracking is not configured.
    fn branches_ahead_behind(
        &self,
        repo: &RepoId,
    ) -> Result<std::collections::BTreeMap<String, AheadBehind>>;

    /// Stash the working-tree changes. With `message` the stash is labelled;
    /// `include_untracked` also stashes untracked files (`-u`).
    fn stash_save(
        &self,
        repo: &RepoId,
        message: Option<&str>,
        include_untracked: bool,
    ) -> Result<()>;

    /// List the stash stack, most recent first (`stash@{0}`).
    fn stash_list(&self, repo: &RepoId) -> Result<Vec<StashEntry>>;

    /// Apply `stash@{index}` to the working tree, keeping it in the stack.
    fn stash_apply(&self, repo: &RepoId, index: usize) -> Result<()>;

    /// Apply `stash@{index}` and drop it from the stack on success.
    fn stash_pop(&self, repo: &RepoId, index: usize) -> Result<()>;

    /// Drop `stash@{index}` without applying it.
    fn stash_drop(&self, repo: &RepoId, index: usize) -> Result<()>;

    /// Merge `source` (a branch/ref name) into the current branch. Reports the
    /// outcome; on conflict the working tree is left mid-merge with the
    /// conflicted paths listed (resolve in Phase 3, or call
    /// [`GitEngine::merge_abort`]).
    fn merge(&self, repo: &RepoId, source: &str, opts: &MergeOpts) -> Result<MergeOutcome>;

    /// Abort an in-progress merge, restoring the pre-merge state
    /// (`git merge --abort`).
    fn merge_abort(&self, repo: &RepoId) -> Result<()>;

    /// Cherry-pick `oid` onto the current branch. On conflict, leaves the
    /// repository mid-sequencer and reports the conflicted paths.
    fn cherry_pick(&self, repo: &RepoId, oid: &Oid) -> Result<ApplyOutcome>;

    /// Revert `oid` onto the current branch. On conflict, leaves the repository
    /// mid-sequencer and reports the conflicted paths.
    fn revert(&self, repo: &RepoId, oid: &Oid) -> Result<ApplyOutcome>;

    /// Abort an in-progress cherry-pick or revert sequencer.
    fn sequencer_abort(&self, repo: &RepoId) -> Result<()>;

    /// Rebase `branch` onto `onto` using non-interactive system git. On
    /// conflict, leaves the repository mid-rebase and reports conflicted paths.
    fn rebase(&self, repo: &RepoId, branch: &str, onto: &str) -> Result<RebaseOutcome>;

    /// Abort an in-progress rebase.
    fn rebase_abort(&self, repo: &RepoId) -> Result<()>;

    /// List the currently conflicted paths from `status()` (sorted, deduped).
    fn list_conflicts(&self, repo: &RepoId) -> Result<Vec<String>>;

    /// Read the working-tree content of a conflicted file (with markers) and
    /// parse it into context + conflict regions (PH3-001 / lady-diff::merge).
    fn parse_conflict(&self, repo: &RepoId, path: &str) -> Result<ParsedConflict>;

    /// Read the three sides of a conflicted `path` from the index stages
    /// (`:1:` base, `:2:` ours, `:3:` theirs). Each is `None` when absent.
    fn conflict_sides(&self, repo: &RepoId, path: &str) -> Result<ConflictSides>;

    /// Resolve `path` by taking our side of every conflict region (parsed from
    /// the working file), writing the result back. Does not stage.
    fn take_ours(&self, repo: &RepoId, path: &str) -> Result<()>;

    /// Resolve `path` by taking their side of every conflict region.
    fn take_theirs(&self, repo: &RepoId, path: &str) -> Result<()>;

    /// Write `bytes` as the resolved content of `path` (the edited result pane).
    fn write_resolution(&self, repo: &RepoId, path: &str, bytes: &[u8]) -> Result<()>;

    /// Mark `path` resolved by staging it (`git add`).
    fn mark_resolved(&self, repo: &RepoId, path: &str) -> Result<()>;

    /// What mid-operation state the repo is in (merge / rebase / cherry-pick /
    /// revert / none), inspected from the git dir.
    fn conflict_state(&self, repo: &RepoId) -> Result<ConflictState>;

    /// Abort whatever operation is in progress, routing to the right
    /// `--abort` per [`GitEngine::conflict_state`]. A no-op when idle.
    fn conflict_abort(&self, repo: &RepoId) -> Result<()>;

    /// Run an interactive rebase onto `onto`, driving git's todo list from
    /// `plan` (a generated todo + `GIT_SEQUENCE_EDITOR` shim, plus a
    /// `GIT_EDITOR` shim feeding reword/squash messages). Stops on conflict or
    /// an `edit` step, leaving the repo mid-rebase for `continue` / `abort`.
    fn rebase_interactive(
        &self,
        repo: &RepoId,
        onto: &str,
        plan: &[RebaseStep],
    ) -> Result<RebaseOutcome>;

    /// Continue an in-progress (interactive) rebase after resolving a conflict
    /// or finishing an `edit` amendment.
    fn rebase_continue(&self, repo: &RepoId) -> Result<RebaseOutcome>;

    /// Skip the current commit of an in-progress rebase.
    fn rebase_skip(&self, repo: &RepoId) -> Result<RebaseOutcome>;

    /// Compute the interactive-rebase range "from `from` to HEAD": returns the
    /// `onto` target (the parent of `from`) and the commits in the range, oldest
    /// first, ready to seed a [`RebaseStep`] plan (PH3-004 entry point).
    fn rebase_range(&self, repo: &RepoId, from: &Oid) -> Result<(Oid, Vec<CommitMeta>)>;

    /// Verification status for each of `oids` (git's `%G?`), in the same order.
    /// One `git log --no-walk` call covers the whole batch (PH3-005); unknown
    /// oids map to [`SignatureStatus::None`].
    fn signature_statuses(&self, repo: &RepoId, oids: &[Oid]) -> Result<Vec<SignatureStatus>>;

    /// List the repository's worktrees (`git worktree list --porcelain`).
    fn list_worktrees(&self, repo: &RepoId) -> Result<Vec<Worktree>>;

    /// Describe the selected repository's family: stable common-git-dir id,
    /// main worktree, and every worktree Git reports for the family.
    fn repository_family(&self, repo: &RepoId) -> Result<RepositoryFamily>;

    /// Add a worktree at `path`. With `new_branch`, create branch `branch`
    /// there (`-b`); otherwise check out the existing `branch` (or a detached
    /// HEAD when `branch` is `None`).
    fn add_worktree(
        &self,
        repo: &RepoId,
        path: &str,
        branch: Option<&str>,
        new_branch: bool,
    ) -> Result<()>;

    /// Remove the worktree at `path` (`git worktree remove`).
    fn remove_worktree(&self, repo: &RepoId, path: &str) -> Result<()>;

    /// Prune stale worktree administrative entries (`git worktree prune`).
    fn prune_worktrees(&self, repo: &RepoId) -> Result<()>;

    /// The reflog for `refname` (e.g. `HEAD`), newest first, for recovering
    /// lost commits (PH3-007).
    fn reflog(&self, repo: &RepoId, refname: &str) -> Result<Vec<ReflogEntry>>;

    /// Start a bisect bounded by a known-`bad` and known-`good` commit; git
    /// checks out a midpoint to test (PH3-008). Returns the resulting state.
    fn bisect_start(&self, repo: &RepoId, bad: &Oid, good: &Oid) -> Result<BisectState>;

    /// Mark the current bisect commit `good`, `bad`, or `skip`; git advances to
    /// the next commit or reports the first bad one. Returns the new state.
    fn bisect_mark(&self, repo: &RepoId, mark: &str) -> Result<BisectState>;

    /// Exit bisect, restoring the original branch (`git bisect reset`).
    fn bisect_reset(&self, repo: &RepoId) -> Result<()>;

    /// The current bisect state (or an empty state when not bisecting).
    fn bisect_state(&self, repo: &RepoId) -> Result<BisectState>;

    /// Run a custom command `argv` against the repo worktree, capturing its
    /// stdout/stderr/exit-code (PH3-009). `argv` is an argument vector (no
    /// shell), built by [`custom::build_argv`] for injection safety.
    fn run_custom(&self, repo: &RepoId, argv: &[String]) -> Result<CommandOutput>;

    /// Launch the user's configured external diff tool (`diff.tool`) on `path`.
    /// With `commit`, diff that commit against its parent; otherwise the working
    /// tree (PH3-010). Surfaces git's message when no tool is configured.
    fn launch_difftool(&self, repo: &RepoId, path: &str, commit: Option<&str>) -> Result<()>;

    /// Launch the user's configured external merge tool (`merge.tool`) on a
    /// conflicted `path` (PH3-010).
    fn launch_mergetool(&self, repo: &RepoId, path: &str) -> Result<()>;

    /// The repository's remote fetch URLs (deduped), for forge detection
    /// (PH3-011).
    fn list_remote_urls(&self, repo: &RepoId) -> Result<Vec<String>>;

    /// Add a remote `name` pointing at `url` (`git remote add`), e.g. wiring a
    /// freshly created remote as `origin` (PH4-005).
    fn add_remote(&self, repo: &RepoId, name: &str, url: &str) -> Result<()>;

    /// Git LFS status: availability, tracked patterns, and tracked files with
    /// their materialized/pointer state (PH4-007). Empty when git-lfs is
    /// unavailable. Clone/fetch/checkout already run smudge/clean filters
    /// because every mutation shells out to the user's git (ADR-0003).
    fn lfs_status(&self, repo: &RepoId) -> Result<LfsStatus>;

    /// Track `pattern` with LFS (`git lfs track <pattern>`), writing
    /// `.gitattributes`. Errors clearly when git-lfs is not installed.
    fn lfs_track(&self, repo: &RepoId, pattern: &str) -> Result<()>;

    /// Read the repo's local git identity (`.git/config` `user.name`/`user.email`).
    fn repo_identity_get(&self, repo: &RepoId) -> Result<GitIdentity>;

    /// Write the repo's local git identity. An empty `name`/`email` unsets that
    /// key. Writes `.git/config --local` (the scoped ADR-0006 carve-out).
    fn repo_identity_set(&self, repo: &RepoId, name: &str, email: &str) -> Result<()>;

    /// Read the persisted git-flow config (`gitflow.*`), or defaults when not
    /// initialized (PH4-008).
    fn flow_config(&self, repo: &RepoId) -> Result<FlowConfig>;

    /// Initialize git-flow: persist the config and create the `develop` branch
    /// from `master` if missing.
    fn flow_init(&self, repo: &RepoId, config: &FlowConfig) -> Result<()>;

    /// Start a flow branch of `kind` named `name`; returns the created branch.
    /// Feature/Release branch from `develop`; Hotfix branches from `master`.
    fn flow_start(&self, repo: &RepoId, kind: FlowKind, name: &str) -> Result<String>;

    /// Finish a flow branch: Feature merges into `develop`; Release/Hotfix merge
    /// into `master` (tagged) and `develop`, then the branch is deleted. Native
    /// git-flow semantics via shell-out git (no git-flow binary required).
    fn flow_finish(&self, repo: &RepoId, kind: FlowKind, name: &str) -> Result<()>;

    /// List submodules with status (`git submodule status --recursive` + URLs
    /// from `.gitmodules`), nested submodules included (PH4-009).
    fn list_submodules(&self, repo: &RepoId) -> Result<Vec<Submodule>>;

    /// Add a submodule at `path` from `url` (`git submodule add`).
    fn add_submodule(&self, repo: &RepoId, url: &str, path: &str) -> Result<()>;

    /// Initialize + check out all submodules (`git submodule update --init
    /// --recursive`).
    fn init_submodules(&self, repo: &RepoId) -> Result<()>;

    /// Update submodules to their pinned commits (`git submodule update
    /// --recursive`).
    fn update_submodules(&self, repo: &RepoId) -> Result<()>;

    /// Sync submodule URLs from `.gitmodules` into config (`git submodule
    /// sync --recursive`).
    fn sync_submodules(&self, repo: &RepoId) -> Result<()>;

    /// Deinitialize the submodule at `path` (`git submodule deinit -f`).
    fn deinit_submodule(&self, repo: &RepoId, path: &str) -> Result<()>;
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

    /// The worktree directory for `id`, needed by every shell-out git mutation.
    /// Errors for a bare repository (no worktree).
    pub(crate) fn workdir(&self, id: &RepoId) -> Result<PathBuf> {
        let repo = self.repo(id)?;
        repo.workdir()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| Error::Git("repository has no worktree (bare)".to_string()))
    }

    /// The repository's working-directory path (public accessor for hosts that
    /// key per-repo state by path, e.g. the AI per-repo toggle, PH5-002).
    pub fn workdir_path(&self, id: &RepoId) -> Result<PathBuf> {
        self.workdir(id)
    }

    /// Canonical common git directory shared by every worktree in a repository
    /// family. This is the durable family id from ADR-0012.
    pub fn repository_family_id(&self, id: &RepoId) -> Result<RepositoryFamilyId> {
        let repo = self.repo(id)?;
        let raw = repo.common_dir().to_path_buf();
        let path = raw.canonicalize().unwrap_or(raw);
        Ok(RepositoryFamilyId::from(path.to_string_lossy().to_string()))
    }

    /// Fetch URL for remote `name` (`git remote get-url`).
    pub fn remote_url(&self, id: &RepoId, name: &str) -> Result<String> {
        let wd = self.workdir(id)?;
        let out = run_git(&wd, &["remote", "get-url", name])?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Read a file's contents at a revision (`<rev>:<path>`), as lossy UTF-8.
    /// `None` when the path does not exist at that rev (PH5-011 get_file_at).
    pub fn file_at(&self, id: &RepoId, rev: &str, path: &str) -> Result<Option<String>> {
        let wd = self.workdir(id)?;
        Ok(cat_blob(&wd, &format!("{rev}:{path}"))
            .map(|b| String::from_utf8_lossy(&b).into_owned()))
    }

    /// Commits whose summary contains `query` (case-insensitive), newest first,
    /// capped at `limit` (0 = no cap). A simple message-grep walk (PH5-011
    /// search_commits); semantic search is PH5-012.
    pub fn search_commits(
        &self,
        id: &RepoId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CommitMeta>> {
        let needle = query.to_lowercase();
        let all = self.walk_log(
            id,
            GraphQuery {
                start: None,
                limit: 0,
            },
        )?;
        let mut hits: Vec<CommitMeta> = all
            .into_iter()
            .filter(|c| c.summary.to_lowercase().contains(&needle))
            .collect();
        if limit > 0 {
            hits.truncate(limit);
        }
        Ok(hits)
    }

    /// Map a rebase process result + the post-run repo state to a
    /// [`RebaseOutcome`]: completed, stopped on conflict, stopped for an `edit`
    /// step, or a hard error (git's message surfaced).
    fn interpret_rebase(&self, repo: &RepoId, out: &std::process::Output) -> Result<RebaseOutcome> {
        // Conflicts take precedence — visible whether git exited zero or not.
        let conflicts = conflict_paths(&self.status(repo)?);
        if !conflicts.is_empty() {
            return Ok(RebaseOutcome::Conflicts(conflicts));
        }
        // Still mid-rebase with no conflict → stopped for an `edit` step.
        if self.conflict_state(repo)? == ConflictState::Rebase {
            return Ok(RebaseOutcome::Stopped);
        }
        if out.status.success() {
            return Ok(RebaseOutcome::Rebased);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        Err(Error::Git(if msg.is_empty() {
            format!("git rebase failed ({})", out.status)
        } else {
            msg.to_string()
        }))
    }

    /// Parse a `git bisect` command's output into a [`BisectState`]: detect the
    /// "first bad commit" verdict, else the current commit + steps estimate.
    fn parse_bisect(&self, workdir: &Path, out: &std::process::Output) -> Result<BisectState> {
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));

        // Converged: "<sha> is the first bad commit".
        if let Some(line) = text.lines().find(|l| l.contains("is the first bad commit")) {
            if let Some(sha) = line.split_whitespace().next() {
                let oid = Oid::from(sha.to_string());
                return Ok(BisectState {
                    current_oid: Some(oid.clone()),
                    remaining_steps_estimate: 0,
                    suspected: Some(oid),
                });
            }
        }

        // Still bisecting: HEAD is the commit under test; estimate is the
        // "roughly N steps" figure git prints.
        Ok(BisectState {
            current_oid: Some(head_oid(workdir)?),
            remaining_steps_estimate: num_after(&text, "roughly ").unwrap_or(0),
            suspected: None,
        })
    }
}

/// Run system `git` in `workdir` with `args`, returning its captured output.
///
/// On a non-zero exit, returns [`Error::Git`] carrying git's stderr verbatim
/// (ADR-0003 shell-out mutation tier; surface git's own messages faithfully).
pub(crate) fn run_git(workdir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let out = run_git_raw(workdir, args)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        return Err(Error::Git(if msg.is_empty() {
            format!("git {args:?} failed ({})", out.status)
        } else {
            msg.to_string()
        }));
    }
    Ok(out)
}

/// Make a git child non-interactive so it never blocks on a terminal credential
/// prompt. Lady runs behind a GUI with no controlling TTY, so a prompt yields the
/// cryptic `could not read Username for 'https://…': Device not configured` (and
/// can hang). Credentials must come from a per-invocation [`GitAuth`] header/
/// helper or a configured credential helper; when none resolve, git now fails
/// fast with a clear auth error instead of reaching for a terminal.
fn make_noninteractive(cmd: &mut Command) {
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    // Git Credential Manager (if it is the configured helper): do not pop UI.
    cmd.env("GCM_INTERACTIVE", "never");
}

/// Run system `git` and return the raw output without mapping non-zero exits.
fn run_git_raw(workdir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.current_dir(workdir).args(args);
    make_noninteractive(&mut cmd);
    cmd.output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))
}

/// Run system `git` with extra environment variables, returning raw output
/// (non-zero exits are not mapped). Used to drive interactive rebase with a
/// `GIT_SEQUENCE_EDITOR` / `GIT_EDITOR` shim (ADR-0003).
fn run_git_env_raw(
    workdir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.current_dir(workdir).args(args);
    make_noninteractive(&mut cmd);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))
}

/// True when `ancestor` is reachable from `descendant`.
fn is_ancestor(workdir: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    let out = run_git_raw(
        workdir,
        &["merge-base", "--is-ancestor", ancestor, descendant],
    )?;
    Ok(out.status.success())
}

/// Current `HEAD` object id.
fn head_oid(workdir: &Path) -> Result<Oid> {
    let out = run_git(workdir, &["rev-parse", "HEAD"])?;
    Ok(Oid::from(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// Deduplicate conflicted paths from a porcelain-derived status snapshot.
fn conflict_paths(wt: &WorkingTree) -> Vec<String> {
    let mut conflicts: Vec<String> = wt
        .staged
        .iter()
        .chain(wt.unstaged.iter())
        .filter(|f| f.kind == ChangeKind::Conflicted)
        .map(|f| f.path.clone())
        .collect();
    conflicts.sort();
    conflicts.dedup();
    conflicts
}

/// Convert a raw cherry-pick/revert result into the shared sequencer outcome.
fn sequencer_outcome(
    workdir: &Path,
    wt: &WorkingTree,
    args: &[&str],
    out: &std::process::Output,
) -> Result<ApplyOutcome> {
    if out.status.success() {
        return Ok(ApplyOutcome::Applied(head_oid(workdir)?));
    }

    let conflicts = conflict_paths(wt);
    if !conflicts.is_empty() {
        return Ok(ApplyOutcome::Conflicts(conflicts));
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    let msg = stderr.trim();
    Err(Error::Git(if msg.is_empty() {
        format!("git {args:?} failed ({})", out.status)
    } else {
        msg.to_string()
    }))
}

/// Run system `git` in `workdir`, feeding `input` to its stdin (used for
/// `git apply`, which reads the patch from stdin). Errors carry git's stderr.
pub(crate) fn run_git_stdin(workdir: &Path, args: &[&str], input: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = Command::new("git");
    cmd.current_dir(workdir).args(args);
    make_noninteractive(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
    child
        .stdin
        .take()
        .expect("git stdin was piped")
        .write_all(input)
        .map_err(|e| Error::Git(format!("failed to write to git stdin: {e}")))?;
    let out = child
        .wait_with_output()
        .map_err(|e| Error::Git(e.to_string()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        return Err(Error::Git(if msg.is_empty() {
            format!("git {args:?} failed ({})", out.status)
        } else {
            msg.to_string()
        }));
    }
    Ok(())
}

/// Run system `git` in `workdir`, streaming progress to `on_line` as each
/// stderr line arrives (git writes `--progress` output to stderr). The same
/// lines are collected so a non-zero exit returns [`Error::Git`] carrying
/// git's own message verbatim — auth failures, non-fast-forward rejections,
/// etc. surface unchanged while progress still streams live (ADR-0003).
pub(crate) fn run_git_streaming(
    workdir: &Path,
    args: &[&str],
    auth: &GitAuth,
    on_line: &mut dyn FnMut(&str),
) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let mut cmd = Command::new("git");
    cmd.current_dir(workdir);
    make_noninteractive(&mut cmd);
    // Per-invocation auth overrides as leading `-c k=v` flags before the
    // subcommand (no-op when `auth` is empty — the default path).
    for (k, v) in &auth.config {
        cmd.arg("-c").arg(format!("{k}={v}"));
    }
    cmd.args(args);
    for (k, v) in &auth.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;

    // Drain stderr line-by-line: feed each to the progress callback and keep a
    // copy so any failure can be reported with git's full message.
    let mut collected = String::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr)
            .lines()
            .map_while(std::result::Result::ok)
        {
            on_line(&line);
            collected.push_str(&line);
            collected.push('\n');
        }
    }

    let status = child
        .wait()
        .map_err(|e| Error::Git(format!("failed to wait on git: {e}")))?;
    if !status.success() {
        let msg = collected.trim();
        return Err(Error::Git(if msg.is_empty() {
            format!("git {args:?} failed ({status})")
        } else {
            msg.to_string()
        }));
    }
    Ok(())
}

/// Short name of the checked-out branch (`HEAD` when detached).
fn current_branch_name(workdir: &Path) -> Result<String> {
    let out = run_git(workdir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name == "HEAD" {
        return Err(Error::Git("cannot push: HEAD is detached".into()));
    }
    Ok(name)
}

/// Whether the current branch has an upstream configured.
fn has_upstream(workdir: &Path) -> bool {
    run_git(workdir, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_ok()
}

/// Remote to push `branch` to: per-branch remote, `remote.pushDefault`, then `origin`.
fn default_push_remote(workdir: &Path, branch: &str) -> Result<String> {
    let branch_remote_key = format!("branch.{branch}.remote");
    if let Ok(out) = run_git(workdir, &["config", "--get", &branch_remote_key]) {
        let r = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !r.is_empty() {
            return Ok(r);
        }
    }
    if let Ok(out) = run_git(workdir, &["config", "--get", "remote.pushDefault"]) {
        let r = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !r.is_empty() {
            return Ok(r);
        }
    }
    if run_git(workdir, &["remote", "get-url", "origin"]).is_ok() {
        return Ok("origin".into());
    }
    Err(Error::Git(format!(
        "no remote configured to push branch '{branch}'"
    )))
}

fn git_ref_exists(workdir: &Path, refname: &str) -> Result<bool> {
    Ok(
        run_git_raw(workdir, &["show-ref", "--verify", "--quiet", refname])?
            .status
            .success(),
    )
}

fn checkout_remote_tracking(workdir: &Path, target: &str, force: bool) -> Result<bool> {
    let exact_local = format!("refs/heads/{target}");
    if git_ref_exists(workdir, &exact_local)? {
        return Ok(false);
    }

    let remote_ref = format!("refs/remotes/{target}");
    if !git_ref_exists(workdir, &remote_ref)? {
        return Ok(false);
    }

    let Some((_, local_branch)) = target.split_once('/') else {
        return Ok(false);
    };
    if local_branch == "HEAD" || local_branch.ends_with("/HEAD") {
        return Err(Error::Git(format!(
            "cannot check out remote HEAD '{target}'; choose a branch"
        )));
    }

    let local_ref = format!("refs/heads/{local_branch}");
    if git_ref_exists(workdir, &local_ref)? {
        let mut args: Vec<&str> = vec!["checkout"];
        if force {
            args.push("--force");
        }
        args.push(local_branch);
        run_git(workdir, &args)?;
        return Ok(true);
    }

    let mut args: Vec<&str> = vec!["checkout"];
    if force {
        args.push("--force");
    }
    args.extend(["--track", "-b", local_branch, target]);
    run_git(workdir, &args)?;
    Ok(true)
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
        // unborn HEAD (fresh repo, no commits) is simply omitted. The ref's
        // `name` is the checked-out branch's short name (e.g. `main`) so the UI
        // can identify the *current* branch even when several branches share the
        // same tip commit; a detached HEAD falls back to the literal `HEAD`.
        if let Ok(head) = repo.head_id() {
            let name = repo
                .head_ref()
                .ok()
                .flatten()
                .map(|r| r.name().shorten().to_string())
                .unwrap_or_else(|| "HEAD".to_string());
            out.push(RefInfo {
                name,
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
        let grepo = self.repo(repo)?;
        let commit = grepo
            .find_commit(gix::ObjectId::from_hex(commit_oid.as_str().as_bytes()).map_err(backend)?)
            .map_err(backend)?;
        let new_tree_id = commit.tree().map_err(backend)?.id;
        // Parent tree (None for root commits ⇒ everything is "added").
        let old_tree_id: Option<gix::ObjectId> = commit
            .parent_ids()
            .next()
            .map(|pid| {
                grepo
                    .find_commit(pid.detach())
                    .map_err(backend)
                    .and_then(|p| Ok(p.tree().map_err(backend)?.id))
            })
            .transpose()?;
        diff_trees(&grepo, old_tree_id, new_tree_id)
    }

    fn diff_range(&self, repo: &RepoId, base: &Oid, head: &Oid) -> Result<Vec<FileDiff>> {
        let grepo = self.repo(repo)?;
        let find_tree = |oid: &Oid| -> Result<gix::ObjectId> {
            let c = grepo
                .find_commit(gix::ObjectId::from_hex(oid.as_str().as_bytes()).map_err(backend)?)
                .map_err(backend)?;
            Ok(c.tree().map_err(backend)?.id)
        };
        let old_tree_id = find_tree(base)?;
        let new_tree_id = find_tree(head)?;
        diff_trees(&grepo, Some(old_tree_id), new_tree_id)
    }

    fn reset(&self, repo: &RepoId, target: &Oid, mode: ResetMode) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["reset", mode.flag(), target.as_str()]).map(|_| ())
    }

    fn head_commit(&self, repo: &RepoId) -> Result<Oid> {
        let wd = self.workdir(repo)?;
        head_oid(&wd)
    }

    fn commit_is_pushed(&self, repo: &RepoId, oid: &Oid) -> Result<bool> {
        let wd = self.workdir(repo)?;
        // `<oid>` is pushed iff it is reachable from the branch's upstream. No
        // upstream (rev-parse fails) ⇒ nothing to compare against ⇒ not pushed.
        if run_git_raw(&wd, &["rev-parse", "--abbrev-ref", "@{u}"])?
            .status
            .success()
        {
            Ok(
                run_git_raw(&wd, &["merge-base", "--is-ancestor", oid.as_str(), "@{u}"])?
                    .status
                    .success(),
            )
        } else {
            Ok(false)
        }
    }

    fn diff_spec(&self, repo: &RepoId, spec: &DiffSpec) -> Result<Vec<FileDiff>> {
        match spec {
            DiffSpec::Commit(oid) => self.diff_commit(repo, oid),
            DiffSpec::WorkingVsIndex(path) => {
                let wd = self.workdir(repo)?;
                // old = index blob (staged content); new = on-disk bytes.
                let old = cat_blob(&wd, &format!(":{path}"));
                let new = std::fs::read(wd.join(path)).ok();
                Ok(single_file_diff(old.as_deref(), new.as_deref(), path))
            }
            DiffSpec::IndexVsHead(path) => {
                let wd = self.workdir(repo)?;
                // old = HEAD blob; new = index blob (staged content).
                let old = cat_blob(&wd, &format!("HEAD:{path}"));
                let new = cat_blob(&wd, &format!(":{path}"));
                Ok(single_file_diff(old.as_deref(), new.as_deref(), path))
            }
        }
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

    fn is_dirty(&self, repo: &RepoId) -> Result<bool> {
        let repo = self.repo(repo)?;
        repo.is_dirty().map_err(backend)
    }

    fn list_files(&self, repo: &RepoId) -> Result<Vec<String>> {
        let repo = self.repo(repo)?;
        let tree_id = repo
            .head_id()
            .map_err(backend)?
            .object()
            .map_err(backend)?
            .try_into_commit()
            .map_err(backend)?
            .tree()
            .map_err(backend)?
            .id;

        let mut blobs: HashMap<String, gix::ObjectId> = HashMap::new();
        collect_tree_blobs(&repo, tree_id, String::new(), &mut blobs)?;

        let mut paths: Vec<String> = blobs.into_keys().collect();
        paths.sort();
        Ok(paths)
    }

    fn status(&self, repo: &RepoId) -> Result<WorkingTree> {
        let wd = self.workdir(repo)?;
        let out = run_git(
            &wd,
            &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        )?;
        Ok(parse_status_v2(&out.stdout))
    }

    fn stage_paths(&self, repo: &RepoId, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let wd = self.workdir(repo)?;
        // `git add -- <paths>` stages adds/modifications/deletions alike.
        let mut args: Vec<&str> = vec!["add", "--"];
        args.extend(paths.iter().map(String::as_str));
        run_git(&wd, &args).map(|_| ())
    }

    fn unstage_paths(&self, repo: &RepoId, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let wd = self.workdir(repo)?;
        // On an unborn branch there is no HEAD to restore against; drop the
        // entries from the index instead. Otherwise restore from HEAD.
        let has_head = self.repo(repo)?.head_id().is_ok();
        let mut args: Vec<&str> = if has_head {
            vec!["restore", "--staged", "--"]
        } else {
            vec!["rm", "-q", "--cached", "--"]
        };
        args.extend(paths.iter().map(String::as_str));
        run_git(&wd, &args).map(|_| ())
    }

    fn apply_patch(&self, repo: &RepoId, patch: &str, reverse: bool, cached: bool) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["apply"];
        if cached {
            args.push("--cached");
        }
        if reverse {
            args.push("--reverse");
        }
        // No file arg → git reads the patch from stdin.
        run_git_stdin(&wd, &args, patch.as_bytes())
    }

    fn discard_untracked(&self, repo: &RepoId, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let wd = self.workdir(repo)?;
        // `-f` force, `-d` also remove untracked directories, `-q` quiet.
        let mut args: Vec<&str> = vec!["clean", "-f", "-d", "-q", "--"];
        args.extend(paths.iter().map(String::as_str));
        run_git(&wd, &args).map(|_| ())
    }

    fn commit(&self, repo: &RepoId, message: &str, opts: &CommitOpts) -> Result<Oid> {
        let wd = self.workdir(repo)?;
        // Pass the message via stdin (`-F -`) so arbitrary text — newlines,
        // quotes, leading dashes — survives without shell quoting. `--amend`
        // rewrites the tip; signing config is inherited, never overridden.
        let mut args: Vec<&str> = vec!["commit", "-F", "-"];
        if opts.amend {
            args.push("--amend");
        }
        // Force a signature when asked; otherwise inherit `commit.gpgsign`.
        if opts.sign {
            args.push("-S");
        }
        run_git_stdin(&wd, &args, message.as_bytes())?;
        // The new tip is the committed Oid.
        let out = run_git(&wd, &["rev-parse", "HEAD"])?;
        let oid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(Oid::from(oid))
    }

    fn recent_messages(&self, repo: &RepoId, limit: usize) -> Result<Vec<String>> {
        // No commits yet → no messages (git log would error on an unborn HEAD).
        if self.repo(repo)?.head_id().is_err() {
            return Ok(Vec::new());
        }
        let wd = self.workdir(repo)?;
        let n = limit.to_string();
        let out = run_git(&wd, &["log", "--format=%s", "-n", &n])?;
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect())
    }

    fn create_branch(&self, repo: &RepoId, name: &str, start_point: Option<&str>) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["branch", "--", name];
        if let Some(sp) = start_point {
            // `--` was for the branch name; re-form with the start point.
            args = vec!["branch", name, sp];
        }
        run_git(&wd, &args).map(|_| ())
    }

    fn delete_branch(&self, repo: &RepoId, name: &str, force: bool) -> Result<()> {
        let wd = self.workdir(repo)?;
        let flag = if force { "-D" } else { "-d" };
        run_git(&wd, &["branch", flag, "--", name]).map(|_| ())
    }

    fn rename_branch(&self, repo: &RepoId, old: &str, new: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["branch", "-m", old, new]).map(|_| ())
    }

    fn branch_upstream(&self, repo: &RepoId, branch: &str) -> Result<Option<String>> {
        let wd = self.workdir(repo)?;
        let refspec = format!("refs/heads/{branch}");
        let out = run_git(
            &wd,
            &["for-each-ref", "--format=%(upstream:short)", &refspec],
        )?;
        let up = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(if up.is_empty() { None } else { Some(up) })
    }

    fn set_branch_upstream(
        &self,
        repo: &RepoId,
        branch: &str,
        upstream: Option<&str>,
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        match upstream {
            Some(up) => {
                let flag = format!("--set-upstream-to={up}");
                run_git(&wd, &["branch", &flag, branch]).map(|_| ())
            }
            None => run_git(&wd, &["branch", "--unset-upstream", branch]).map(|_| ()),
        }
    }

    fn fast_forward_branch(&self, repo: &RepoId, branch: &str, upstream: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        // Refuse non-fast-forward moves: branch must be an ancestor of upstream.
        let anc = run_git_raw(&wd, &["merge-base", "--is-ancestor", branch, upstream])?;
        if !anc.status.success() {
            return Err(Error::Git(format!(
                "{branch} is not behind {upstream} (not fast-forwardable)"
            )));
        }
        let refspec = format!("refs/heads/{branch}");
        run_git(&wd, &["update-ref", &refspec, upstream]).map(|_| ())
    }

    fn discard_files(&self, repo: &RepoId, paths: &[String]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["checkout", "HEAD", "--"];
        args.extend(paths.iter().map(String::as_str));
        run_git(&wd, &args).map(|_| ())
    }

    fn stash_paths(
        &self,
        repo: &RepoId,
        message: Option<&str>,
        include_untracked: bool,
        paths: &[String],
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["stash", "push"];
        if include_untracked {
            args.push("-u");
        }
        if let Some(m) = message {
            args.push("-m");
            args.push(m);
        }
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
        run_git(&wd, &args).map(|_| ())
    }

    fn export_patch(&self, repo: &RepoId, paths: &[String], dest: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["diff", "HEAD", "--"];
        args.extend(paths.iter().map(String::as_str));
        let out = run_git(&wd, &args)?;
        std::fs::write(dest, &out.stdout)
            .map_err(|e| Error::Git(format!("failed to write patch {dest}: {e}")))
    }

    fn add_to_gitignore(&self, repo: &RepoId, patterns: &[String]) -> Result<()> {
        if patterns.is_empty() {
            return Ok(());
        }
        let wd = self.workdir(repo)?;
        let path = wd.join(".gitignore");
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let mut body = existing.clone();
        // Keep a trailing newline before appending so entries never merge onto
        // a prior unterminated line.
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        for p in patterns {
            // Skip patterns already present (exact-line match) to avoid dupes.
            let present = existing.lines().any(|l| l == p);
            if !present {
                body.push_str(p);
                body.push('\n');
            }
        }
        std::fs::write(&path, body)
            .map_err(|e| Error::Git(format!("failed to write .gitignore: {e}")))
    }

    fn resolve_path(&self, repo: &RepoId, path: &str) -> Result<PathBuf> {
        Ok(self.workdir(repo)?.join(path))
    }

    fn checkout(&self, repo: &RepoId, target: &str, force: bool) -> Result<()> {
        let wd = self.workdir(repo)?;
        if checkout_remote_tracking(&wd, target, force)? {
            return Ok(());
        }
        // Without `--force`, git refuses to overwrite local changes and prints a
        // clear message — surfaced verbatim by run_git's Error::Git.
        let mut args: Vec<&str> = vec!["checkout"];
        if force {
            args.push("--force");
        }
        args.push(target);
        run_git(&wd, &args).map(|_| ())
    }

    fn create_tag(
        &self,
        repo: &RepoId,
        name: &str,
        target: Option<&str>,
        message: Option<&str>,
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["tag"];
        // Annotated when a message is given (`-m`), else lightweight.
        if let Some(m) = message {
            args.push("-a");
            args.push("-m");
            args.push(m);
        }
        args.push(name);
        if let Some(t) = target {
            args.push(t);
        }
        run_git(&wd, &args).map(|_| ())
    }

    fn delete_tag(&self, repo: &RepoId, name: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["tag", "-d", name]).map(|_| ())
    }

    fn move_tag(&self, repo: &RepoId, name: &str, target: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        // `git tag -f <name> <target>` updates the ref. This drops any annotation
        // and creates a lightweight tag at the new target, which matches the
        // "fast-forward tag" semantics in the UI.
        run_git(&wd, &["tag", "-f", name, target]).map(|_| ())
    }

    fn fetch(
        &self,
        repo: &RepoId,
        remote: Option<&str>,
        auth: &GitAuth,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["fetch", "--progress"];
        if let Some(r) = remote {
            args.push(r);
        }
        run_git_streaming(&wd, &args, auth, on_progress)
    }

    fn pull(
        &self,
        repo: &RepoId,
        remote: Option<&str>,
        branch: Option<&str>,
        auth: &GitAuth,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["pull", "--progress"];
        // A branch can only be named alongside its remote.
        if let Some(r) = remote {
            args.push(r);
            if let Some(b) = branch {
                args.push(b);
            }
        }
        run_git_streaming(&wd, &args, auth, on_progress)
    }

    fn push(
        &self,
        repo: &RepoId,
        remote: Option<&str>,
        branch: Option<&str>,
        set_upstream: bool,
        force: bool,
        auth: &GitAuth,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut remote_owned = remote.map(str::to_string);
        let mut branch_owned = branch.map(str::to_string);

        // `git push -u` (and the first push of a branch with no upstream) needs
        // an explicit `remote branch` refspec — git won't infer one from `-u` alone.
        let upstream = has_upstream(&wd);
        let need_target =
            set_upstream || !upstream || remote_owned.is_some() || branch_owned.is_some();
        if need_target && (remote_owned.is_none() || branch_owned.is_none()) {
            let branch_name = match branch_owned.clone() {
                Some(b) => b,
                None => current_branch_name(&wd)?,
            };
            if branch_owned.is_none() {
                branch_owned = Some(branch_name.clone());
            }
            if remote_owned.is_none() {
                remote_owned = Some(default_push_remote(&wd, &branch_name)?);
            }
        }

        let mut args: Vec<&str> = vec!["push", "--progress"];
        if set_upstream {
            args.push("-u");
        }
        if force {
            args.push("--force");
        }
        if let Some(r) = remote_owned.as_deref() {
            args.push(r);
            if let Some(b) = branch_owned.as_deref() {
                args.push(b);
            }
        }
        run_git_streaming(&wd, &args, auth, on_progress)
    }

    fn ahead_behind(&self, repo: &RepoId) -> Result<Option<AheadBehind>> {
        let wd = self.workdir(repo)?;
        // No upstream → nothing to compare against. `rev-parse` fails loudly,
        // which here just means "untracked branch", so map that to `None`.
        if run_git(&wd, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_err() {
            return Ok(None);
        }
        let out = run_git(
            &wd,
            &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
        )?;
        let text = String::from_utf8_lossy(&out.stdout);
        // `git rev-list --left-right --count A...B` prints "<behind>\t<ahead>".
        let mut nums = text.split_whitespace();
        let behind = nums.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let ahead = nums.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        Ok(Some(AheadBehind { ahead, behind }))
    }

    fn branches_ahead_behind(
        &self,
        repo: &RepoId,
    ) -> Result<std::collections::BTreeMap<String, AheadBehind>> {
        let wd = self.workdir(repo)?;
        // One call lists each local branch with its upstream short name.
        let out = run_git(
            &wd,
            &[
                "for-each-ref",
                "--format=%(refname:short)%00%(upstream:short)",
                "refs/heads",
            ],
        )?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut map = std::collections::BTreeMap::new();
        let mut local_branches = std::collections::BTreeSet::new();
        let mut counted_remotes = std::collections::BTreeSet::new();
        for line in text.lines() {
            let mut parts = line.split('\u{0}');
            let (Some(name), Some(upstream)) = (parts.next(), parts.next()) else {
                continue;
            };
            local_branches.insert(name.to_string());
            if upstream.is_empty() {
                continue;
            }
            // "<behind>\t<ahead>" for upstream...branch (see `ahead_behind`).
            let range = format!("{upstream}...{name}");
            let Ok(counts) = run_git(&wd, &["rev-list", "--left-right", "--count", &range]) else {
                continue;
            };
            let ctext = String::from_utf8_lossy(&counts.stdout);
            let mut nums = ctext.split_whitespace();
            let behind = nums.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let ahead = nums.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let counts = AheadBehind { ahead, behind };
            map.insert(name.to_string(), counts);
            map.insert(upstream.to_string(), counts);
            counted_remotes.insert(upstream.to_string());
        }

        // Also show counts beside remote-tracking branches that have a same
        // named local branch but no explicit upstream relationship configured.
        let remotes = run_git(
            &wd,
            &["for-each-ref", "--format=%(refname:short)", "refs/remotes"],
        )?;
        let text = String::from_utf8_lossy(&remotes.stdout);
        for remote in text.lines().filter(|r| !r.ends_with("/HEAD")) {
            if counted_remotes.contains(remote) {
                continue;
            }
            let Some((_, local)) = remote.split_once('/') else {
                continue;
            };
            if !local_branches.contains(local) {
                continue;
            }
            let range = format!("{remote}...{local}");
            let Ok(counts) = run_git(&wd, &["rev-list", "--left-right", "--count", &range]) else {
                continue;
            };
            let ctext = String::from_utf8_lossy(&counts.stdout);
            let mut nums = ctext.split_whitespace();
            let behind = nums.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let ahead = nums.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            map.insert(remote.to_string(), AheadBehind { ahead, behind });
        }
        Ok(map)
    }

    fn stash_save(
        &self,
        repo: &RepoId,
        message: Option<&str>,
        include_untracked: bool,
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["stash", "push"];
        if include_untracked {
            args.push("-u");
        }
        if let Some(m) = message {
            args.push("-m");
            args.push(m);
        }
        run_git(&wd, &args).map(|_| ())
    }

    fn stash_list(&self, repo: &RepoId) -> Result<Vec<StashEntry>> {
        let wd = self.workdir(repo)?;
        // `%gd` is the selector (stash@{N}), %H the commit, %gs the subject.
        let out = run_git(&wd, &["stash", "list", "--format=%gd%x00%H%x00%gs"])?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut entries = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let mut parts = line.split('\u{0}');
            let _selector = parts.next();
            let oid = parts.next().unwrap_or("").to_string();
            let message = parts.next().unwrap_or("").to_string();
            // The reflog is already newest-first; the line position is the index.
            entries.push(StashEntry {
                index,
                message,
                oid: Oid::from(oid),
            });
        }
        Ok(entries)
    }

    fn stash_apply(&self, repo: &RepoId, index: usize) -> Result<()> {
        let wd = self.workdir(repo)?;
        let sel = format!("stash@{{{index}}}");
        run_git(&wd, &["stash", "apply", &sel]).map(|_| ())
    }

    fn stash_pop(&self, repo: &RepoId, index: usize) -> Result<()> {
        let wd = self.workdir(repo)?;
        let sel = format!("stash@{{{index}}}");
        run_git(&wd, &["stash", "pop", &sel]).map(|_| ())
    }

    fn stash_drop(&self, repo: &RepoId, index: usize) -> Result<()> {
        let wd = self.workdir(repo)?;
        let sel = format!("stash@{{{index}}}");
        run_git(&wd, &["stash", "drop", &sel]).map(|_| ())
    }

    fn merge(&self, repo: &RepoId, source: &str, opts: &MergeOpts) -> Result<MergeOutcome> {
        let wd = self.workdir(repo)?;
        let before = head_oid(&wd)?;
        let target = run_git(&wd, &["rev-parse", source])?;
        let target = String::from_utf8_lossy(&target.stdout).trim().to_string();

        if is_ancestor(&wd, &target, "HEAD")? {
            return Ok(MergeOutcome::AlreadyUpToDate);
        }

        let mut args: Vec<&str> = vec!["merge"];
        match opts.fast_forward {
            FfMode::Auto => args.push("--ff"),
            FfMode::Only => args.push("--ff-only"),
            FfMode::Never => args.push("--no-ff"),
        }
        if let Some(msg) = opts.commit_message.as_deref() {
            args.push("-m");
            args.push(msg);
        } else {
            args.push("--no-edit");
        }
        args.push("--");
        args.push(source);

        let out = run_git_raw(&wd, &args)?;
        if !out.status.success() {
            let conflicts = conflict_paths(&self.status(repo)?);
            if !conflicts.is_empty() {
                return Ok(MergeOutcome::Conflicts(conflicts));
            }

            let stderr = String::from_utf8_lossy(&out.stderr);
            let msg = stderr.trim();
            return Err(Error::Git(if msg.is_empty() {
                format!("git {args:?} failed ({})", out.status)
            } else {
                msg.to_string()
            }));
        }

        let after = head_oid(&wd)?;
        if after == before {
            return Ok(MergeOutcome::AlreadyUpToDate);
        }

        let parent_count = run_git(&wd, &["rev-list", "--parents", "-n", "1", "HEAD"])?;
        let parent_count = String::from_utf8_lossy(&parent_count.stdout)
            .split_whitespace()
            .skip(1)
            .count();
        if parent_count > 1 {
            Ok(MergeOutcome::Merged(after))
        } else {
            Ok(MergeOutcome::FastForwarded)
        }
    }

    fn merge_abort(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["merge", "--abort"]).map(|_| ())
    }

    fn cherry_pick(&self, repo: &RepoId, oid: &Oid) -> Result<ApplyOutcome> {
        let wd = self.workdir(repo)?;
        let args = ["cherry-pick", "--no-edit", oid.as_str()];
        let out = run_git_raw(&wd, &args)?;
        sequencer_outcome(&wd, &self.status(repo)?, &args, &out)
    }

    fn revert(&self, repo: &RepoId, oid: &Oid) -> Result<ApplyOutcome> {
        let wd = self.workdir(repo)?;
        let args = ["revert", "--no-edit", oid.as_str()];
        let out = run_git_raw(&wd, &args)?;
        sequencer_outcome(&wd, &self.status(repo)?, &args, &out)
    }

    fn sequencer_abort(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        if run_git(&wd, &["cherry-pick", "--abort"]).is_ok() {
            return Ok(());
        }
        run_git(&wd, &["revert", "--abort"]).map(|_| ())
    }

    fn rebase(&self, repo: &RepoId, branch: &str, onto: &str) -> Result<RebaseOutcome> {
        let wd = self.workdir(repo)?;
        let args = ["rebase", onto, branch];
        let out = run_git_raw(&wd, &args)?;
        if out.status.success() {
            return Ok(RebaseOutcome::Rebased);
        }

        let conflicts = conflict_paths(&self.status(repo)?);
        if !conflicts.is_empty() {
            return Ok(RebaseOutcome::Conflicts(conflicts));
        }

        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        Err(Error::Git(if msg.is_empty() {
            format!("git {args:?} failed ({})", out.status)
        } else {
            msg.to_string()
        }))
    }

    fn rebase_abort(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["rebase", "--abort"]).map(|_| ())
    }

    fn list_conflicts(&self, repo: &RepoId) -> Result<Vec<String>> {
        Ok(conflict_paths(&self.status(repo)?))
    }

    fn parse_conflict(&self, repo: &RepoId, path: &str) -> Result<ParsedConflict> {
        let wd = self.workdir(repo)?;
        let bytes = std::fs::read(wd.join(path))
            .map_err(|e| Error::Git(format!("failed to read {path}: {e}")))?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(lady_diff::merge::parse_conflicts(&text))
    }

    fn conflict_sides(&self, repo: &RepoId, path: &str) -> Result<ConflictSides> {
        let wd = self.workdir(repo)?;
        // Index stages: 1=base, 2=ours, 3=theirs. Absent stages → None.
        let read_stage = |stage: u8| -> Option<String> {
            cat_blob(&wd, &format!(":{stage}:{path}"))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        Ok(ConflictSides {
            base: read_stage(1),
            ours: read_stage(2),
            theirs: read_stage(3),
        })
    }

    fn take_ours(&self, repo: &RepoId, path: &str) -> Result<()> {
        let parsed = self.parse_conflict(repo, path)?;
        let resolved = lady_diff::merge::resolve(&parsed, true);
        self.write_resolution(repo, path, resolved.as_bytes())
    }

    fn take_theirs(&self, repo: &RepoId, path: &str) -> Result<()> {
        let parsed = self.parse_conflict(repo, path)?;
        let resolved = lady_diff::merge::resolve(&parsed, false);
        self.write_resolution(repo, path, resolved.as_bytes())
    }

    fn write_resolution(&self, repo: &RepoId, path: &str, bytes: &[u8]) -> Result<()> {
        let wd = self.workdir(repo)?;
        std::fs::write(wd.join(path), bytes)
            .map_err(|e| Error::Git(format!("failed to write {path}: {e}")))
    }

    fn mark_resolved(&self, repo: &RepoId, path: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["add", "--", path]).map(|_| ())
    }

    fn conflict_state(&self, repo: &RepoId) -> Result<ConflictState> {
        let git_dir = self.repo(repo)?.git_dir().to_path_buf();
        // Rebase (interactive or apply) takes precedence — it can re-enter
        // cherry-pick-like states internally.
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            Ok(ConflictState::Rebase)
        } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
            Ok(ConflictState::CherryPick)
        } else if git_dir.join("REVERT_HEAD").exists() {
            Ok(ConflictState::Revert)
        } else if git_dir.join("MERGE_HEAD").exists() {
            Ok(ConflictState::Merge)
        } else {
            Ok(ConflictState::None)
        }
    }

    fn conflict_abort(&self, repo: &RepoId) -> Result<()> {
        match self.conflict_state(repo)? {
            ConflictState::Merge => self.merge_abort(repo),
            ConflictState::Rebase => self.rebase_abort(repo),
            ConflictState::CherryPick | ConflictState::Revert => self.sequencer_abort(repo),
            ConflictState::None => Ok(()),
        }
    }

    fn rebase_interactive(
        &self,
        repo: &RepoId,
        onto: &str,
        plan: &[RebaseStep],
    ) -> Result<RebaseOutcome> {
        let wd = self.workdir(repo)?;

        // Scratch dir holding the generated todo, the editor shim, and the
        // ordered reword/squash message files. Kept alive across the git run.
        let dir = tempfile::tempdir().map_err(|e| Error::Git(format!("tempdir: {e}")))?;

        // 1) The rebase todo: one `<keyword> <oid>` line per step, in vec order
        //    (order = the reordering the user chose).
        let mut todo = String::new();
        for step in plan {
            todo.push_str(step.action.keyword());
            todo.push(' ');
            todo.push_str(step.oid.as_str());
            todo.push('\n');
        }
        let todo_path = dir.path().join("todo");
        std::fs::write(&todo_path, &todo).map_err(|e| Error::Git(format!("write todo: {e}")))?;

        // 2) Message files for editor-invoking steps (reword, squash), indexed
        //    by the order git will open the editor (todo order). A missing file
        //    for an index means "leave git's default message".
        let msg_dir = dir.path().join("msgs");
        std::fs::create_dir_all(&msg_dir).map_err(|e| Error::Git(format!("msg dir: {e}")))?;
        let mut editor_idx = 0usize;
        for step in plan {
            if matches!(
                step.action,
                lady_proto::RebaseAction::Reword | lady_proto::RebaseAction::Squash
            ) {
                if let Some(m) = &step.message {
                    std::fs::write(msg_dir.join(format!("msg_{editor_idx}")), m)
                        .map_err(|e| Error::Git(format!("write msg: {e}")))?;
                }
                editor_idx += 1;
            }
        }

        // 3) The GIT_EDITOR shim: pops the next queued message (by a counter in
        //    LADY_MSG_DIR) into the file git wants edited; leaves it untouched
        //    when no message is queued for that invocation.
        let shim_path = dir.path().join("editor.sh");
        let shim = "#!/bin/sh\n\
N=$(cat \"$LADY_MSG_DIR/counter\" 2>/dev/null || echo 0)\n\
M=\"$LADY_MSG_DIR/msg_$N\"\n\
[ -f \"$M\" ] && cp \"$M\" \"$1\"\n\
echo $((N + 1)) > \"$LADY_MSG_DIR/counter\"\n\
exit 0\n";
        std::fs::write(&shim_path, shim).map_err(|e| Error::Git(format!("write shim: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&shim_path)
                .map_err(|e| Error::Git(format!("stat shim: {e}")))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&shim_path, perms)
                .map_err(|e| Error::Git(format!("chmod shim: {e}")))?;
        }

        // `cp <our todo>` as the sequence editor overwrites git's todo file with
        // ours; the shim feeds messages. Both paths are quoted for safety.
        let seq_editor = format!("cp '{}'", todo_path.display());
        let editor = format!("sh '{}'", shim_path.display());
        let msg_dir_str = msg_dir.to_string_lossy().into_owned();
        let envs = [
            ("GIT_SEQUENCE_EDITOR", seq_editor.as_str()),
            ("GIT_EDITOR", editor.as_str()),
            ("LADY_MSG_DIR", msg_dir_str.as_str()),
        ];

        let out = run_git_env_raw(&wd, &["rebase", "-i", onto], &envs)?;
        // Hold the scratch dir until git has finished reading the shim/todo.
        drop(dir);
        self.interpret_rebase(repo, &out)
    }

    fn rebase_continue(&self, repo: &RepoId) -> Result<RebaseOutcome> {
        let wd = self.workdir(repo)?;
        // Accept default messages on continue so squash/reword never blocks.
        let envs = [("GIT_EDITOR", "true"), ("GIT_SEQUENCE_EDITOR", "true")];
        let out = run_git_env_raw(&wd, &["rebase", "--continue"], &envs)?;
        self.interpret_rebase(repo, &out)
    }

    fn rebase_skip(&self, repo: &RepoId) -> Result<RebaseOutcome> {
        let wd = self.workdir(repo)?;
        let envs = [("GIT_EDITOR", "true"), ("GIT_SEQUENCE_EDITOR", "true")];
        let out = run_git_env_raw(&wd, &["rebase", "--skip"], &envs)?;
        self.interpret_rebase(repo, &out)
    }

    fn rebase_range(&self, repo: &RepoId, from: &Oid) -> Result<(Oid, Vec<CommitMeta>)> {
        let wd = self.workdir(repo)?;
        let parent_rev = format!("{}^", from.as_str());
        // `onto` is the parent of `from`; a root commit has none.
        let onto = run_git(&wd, &["rev-parse", &parent_rev])?;
        let onto = Oid::from(String::from_utf8_lossy(&onto.stdout).trim().to_string());

        // Commits in (from^..HEAD], oldest first — the order a todo lists them.
        let range = format!("{parent_rev}..HEAD");
        let out = run_git(&wd, &["rev-list", "--reverse", &range])?;
        let repo_h = self.repo(repo)?;
        let mut commits = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let oid = gix::ObjectId::from_hex(line.trim().as_bytes()).map_err(backend)?;
            let commit = repo_h.find_commit(oid).map_err(backend)?;
            commits.push(commit_meta(&commit)?);
        }
        Ok((onto, commits))
    }

    fn signature_statuses(&self, repo: &RepoId, oids: &[Oid]) -> Result<Vec<SignatureStatus>> {
        if oids.is_empty() {
            return Ok(Vec::new());
        }
        let wd = self.workdir(repo)?;
        // One `git log --no-walk` over all requested commits: `%H<TAB>%G?` per
        // line. `--no-walk=unsorted` keeps each named commit (no ancestry walk).
        let mut args: Vec<&str> = vec!["log", "--no-walk=unsorted", "--format=%H%x09%G?"];
        args.extend(oids.iter().map(|o| o.as_str()));
        let out = run_git(&wd, &args)?;
        let text = String::from_utf8_lossy(&out.stdout);

        let mut by_oid: HashMap<&str, SignatureStatus> = HashMap::new();
        for line in text.lines() {
            let mut parts = line.splitn(2, '\t');
            if let (Some(hash), Some(code)) = (parts.next(), parts.next()) {
                by_oid.insert(hash, SignatureStatus::from_code(code));
            }
        }
        Ok(oids
            .iter()
            .map(|o| {
                by_oid
                    .get(o.as_str())
                    .copied()
                    .unwrap_or(SignatureStatus::None)
            })
            .collect())
    }

    fn list_worktrees(&self, repo: &RepoId) -> Result<Vec<Worktree>> {
        let wd = self.workdir(repo)?;
        let out = run_git(&wd, &["worktree", "list", "--porcelain"])?;
        Ok(enrich_worktrees(
            parse_worktrees(&String::from_utf8_lossy(&out.stdout)),
            &wd,
        ))
    }

    fn repository_family(&self, repo: &RepoId) -> Result<RepositoryFamily> {
        let id = self.repository_family_id(repo)?;
        let worktrees = self.list_worktrees(repo)?;
        let main = worktrees
            .iter()
            .find(|wt| wt.is_main)
            .or_else(|| worktrees.first())
            .cloned()
            .ok_or_else(|| Error::Git("git reported no worktrees".to_string()))?;
        Ok(RepositoryFamily {
            id,
            main,
            worktrees,
        })
    }

    fn add_worktree(
        &self,
        repo: &RepoId,
        path: &str,
        branch: Option<&str>,
        new_branch: bool,
    ) -> Result<()> {
        let wd = self.workdir(repo)?;
        let mut args: Vec<&str> = vec!["worktree", "add"];
        match (branch, new_branch) {
            // New branch created in the worktree: `add -b <branch> <path>`.
            (Some(b), true) => {
                args.push("-b");
                args.push(b);
                args.push(path);
            }
            // Check out an existing branch: `add <path> <branch>`.
            (Some(b), false) => {
                args.push(path);
                args.push(b);
            }
            // Detached / default: `add <path>`.
            (None, _) => args.push(path),
        }
        run_git(&wd, &args).map(|_| ())
    }

    fn remove_worktree(&self, repo: &RepoId, path: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["worktree", "remove", path]).map(|_| ())
    }

    fn prune_worktrees(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["worktree", "prune"]).map(|_| ())
    }

    fn reflog(&self, repo: &RepoId, refname: &str) -> Result<Vec<ReflogEntry>> {
        let wd = self.workdir(repo)?;
        // `%H<TAB>%gd<TAB>%gs` with --date=unix: new-oid, "ref@{<unix>}", and
        // the reflog subject ("action: message"). No reflog → empty stdout.
        let out = run_git(
            &wd,
            &[
                "reflog",
                "show",
                refname,
                "--date=unix",
                "--format=%H%x09%gd%x09%gs",
            ],
        )?;
        let text = String::from_utf8_lossy(&out.stdout);

        let mut entries: Vec<ReflogEntry> = Vec::new();
        for line in text.lines() {
            let mut parts = line.splitn(3, '\t');
            let oid = parts.next().unwrap_or("").to_string();
            let selector = parts.next().unwrap_or("");
            let subject = parts.next().unwrap_or("");

            // Pull the unix time out of `ref@{<unix>}`.
            let time = selector
                .rsplit_once("@{")
                .and_then(|(_, rest)| rest.strip_suffix('}'))
                .and_then(|n| n.parse::<i64>().ok())
                .unwrap_or(0);

            // Split "action: message"; a bare subject is all action.
            let (action, message) = match subject.split_once(": ") {
                Some((a, m)) => (a.to_string(), m.to_string()),
                None => (subject.to_string(), String::new()),
            };

            entries.push(ReflogEntry {
                oid: Oid::from(oid),
                prev_oid: Oid::from(String::new()),
                action,
                message,
                time,
            });
        }

        // The previous value of each entry is the next (older) entry's oid.
        for i in 0..entries.len() {
            let prev = entries
                .get(i + 1)
                .map(|e| e.oid.clone())
                .unwrap_or_else(|| Oid::from(String::new()));
            entries[i].prev_oid = prev;
        }
        Ok(entries)
    }

    fn bisect_start(&self, repo: &RepoId, bad: &Oid, good: &Oid) -> Result<BisectState> {
        let wd = self.workdir(repo)?;
        let out = run_git(&wd, &["bisect", "start", bad.as_str(), good.as_str()])?;
        self.parse_bisect(&wd, &out)
    }

    fn bisect_mark(&self, repo: &RepoId, mark: &str) -> Result<BisectState> {
        // Only the three sequencer verbs are valid here.
        let verb = match mark {
            "good" | "bad" | "skip" => mark,
            other => return Err(Error::Git(format!("invalid bisect mark: {other}"))),
        };
        let wd = self.workdir(repo)?;
        let out = run_git(&wd, &["bisect", verb])?;
        self.parse_bisect(&wd, &out)
    }

    fn bisect_reset(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["bisect", "reset"]).map(|_| ())
    }

    fn bisect_state(&self, repo: &RepoId) -> Result<BisectState> {
        let git_dir = self.repo(repo)?.git_dir().to_path_buf();
        // Not bisecting → empty state.
        if !git_dir.join("BISECT_START").exists() {
            return Ok(BisectState::default());
        }
        let wd = self.workdir(repo)?;
        Ok(BisectState {
            current_oid: Some(head_oid(&wd)?),
            remaining_steps_estimate: 0,
            suspected: None,
        })
    }

    fn run_custom(&self, repo: &RepoId, argv: &[String]) -> Result<CommandOutput> {
        let wd = self.workdir(repo)?;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| Error::Git("empty command".to_string()))?;
        // Argument vector — never a shell string — so user input can't inject.
        let out = Command::new(program)
            .args(args)
            .current_dir(&wd)
            .output()
            .map_err(|e| Error::Git(format!("failed to run '{program}': {e}")))?;
        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            exit_code: out.status.code().unwrap_or(-1),
        })
    }

    fn launch_difftool(&self, repo: &RepoId, path: &str, commit: Option<&str>) -> Result<()> {
        let wd = self.workdir(repo)?;
        // `--no-prompt` skips the per-file confirmation (no terminal in the GUI).
        // diff.tool is honored automatically.
        let mut args: Vec<String> = vec!["difftool".into(), "--no-prompt".into()];
        if let Some(c) = commit {
            args.push(format!("{c}~1"));
            args.push(c.to_string());
        }
        args.push("--".into());
        args.push(path.to_string());
        let argref: Vec<&str> = args.iter().map(String::as_str).collect();
        run_git(&wd, &argref).map(|_| ())
    }

    fn launch_mergetool(&self, repo: &RepoId, path: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        // merge.tool is honored automatically; `--no-prompt` avoids the
        // "Hit return to start tool" prompt.
        run_git(&wd, &["mergetool", "--no-prompt", path]).map(|_| ())
    }

    fn list_remote_urls(&self, repo: &RepoId) -> Result<Vec<String>> {
        let wd = self.workdir(repo)?;
        // `remote.<name>.url` config entries → the fetch URLs. `--get-regexp`
        // exits non-zero when there are no matches; treat that as "no remotes".
        let out = run_git_raw(&wd, &["config", "--get-regexp", r"^remote\..*\.url$"])?;
        if !out.status.success() {
            return Ok(Vec::new());
        }
        let mut urls = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            // "remote.origin.url <url>" → take the value after the first space.
            if let Some((_, url)) = line.split_once(' ') {
                let url = url.trim().to_string();
                if !url.is_empty() && !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }
        Ok(urls)
    }

    fn add_remote(&self, repo: &RepoId, name: &str, url: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["remote", "add", name, url]).map(|_| ())
    }

    fn lfs_status(&self, repo: &RepoId) -> Result<LfsStatus> {
        if !lfs_available() {
            return Ok(LfsStatus::default());
        }
        let wd = self.workdir(repo)?;

        // Tracked patterns: `git lfs track` lists them, one per indented line
        // like "    *.bin (.gitattributes)".
        let patterns = run_git_raw(&wd, &["lfs", "track"])
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .map(|text| {
                text.lines()
                    .filter(|l| l.starts_with(char::is_whitespace) && l.contains(" ("))
                    .filter_map(|l| l.trim().split(" (").next().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Tracked files: `git lfs ls-files` prints "<oid> <*|-> <path>" where
        // `*` = materialized, `-` = pointer only.
        let files = run_git_raw(&wd, &["lfs", "ls-files"])
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .map(|text| {
                text.lines()
                    .filter_map(|line| {
                        let mut parts = line.split_whitespace();
                        let oid = parts.next()?.to_string();
                        let marker = parts.next()?;
                        let path: Vec<&str> = parts.collect();
                        if path.is_empty() {
                            return None;
                        }
                        Some(LfsFile {
                            path: path.join(" "),
                            oid,
                            downloaded: marker == "*",
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(LfsStatus {
            available: true,
            patterns,
            files,
        })
    }

    fn lfs_track(&self, repo: &RepoId, pattern: &str) -> Result<()> {
        if !lfs_available() {
            return Err(Error::Git(
                "git-lfs is not installed. Install it from https://git-lfs.com to track files."
                    .to_string(),
            ));
        }
        let wd = self.workdir(repo)?;
        run_git(&wd, &["lfs", "track", pattern]).map(|_| ())
    }

    fn repo_identity_get(&self, repo: &RepoId) -> Result<GitIdentity> {
        let wd = self.workdir(repo)?;
        Ok(GitIdentity {
            name: git_config_get_local(&wd, "user.name"),
            email: git_config_get_local(&wd, "user.email"),
        })
    }

    fn repo_identity_set(&self, repo: &RepoId, name: &str, email: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        set_or_unset_local(&wd, "user.name", name)?;
        set_or_unset_local(&wd, "user.email", email)?;
        Ok(())
    }

    fn flow_config(&self, repo: &RepoId) -> Result<FlowConfig> {
        let wd = self.workdir(repo)?;
        let get = |key: &str| git_config_get(&wd, key);
        let d = FlowConfig::default();
        let develop = get("gitflow.branch.develop");
        Ok(FlowConfig {
            // Initialized when the develop branch is configured.
            initialized: develop.is_some(),
            master: get("gitflow.branch.master").unwrap_or(d.master),
            develop: develop.unwrap_or(d.develop),
            feature_prefix: get("gitflow.prefix.feature").unwrap_or(d.feature_prefix),
            release_prefix: get("gitflow.prefix.release").unwrap_or(d.release_prefix),
            hotfix_prefix: get("gitflow.prefix.hotfix").unwrap_or(d.hotfix_prefix),
            version_tag_prefix: get("gitflow.prefix.versiontag").unwrap_or(d.version_tag_prefix),
        })
    }

    fn flow_init(&self, repo: &RepoId, config: &FlowConfig) -> Result<()> {
        let wd = self.workdir(repo)?;
        // Persist the config keys.
        let sets = [
            ("gitflow.branch.master", config.master.as_str()),
            ("gitflow.branch.develop", config.develop.as_str()),
            ("gitflow.prefix.feature", config.feature_prefix.as_str()),
            ("gitflow.prefix.release", config.release_prefix.as_str()),
            ("gitflow.prefix.hotfix", config.hotfix_prefix.as_str()),
            (
                "gitflow.prefix.versiontag",
                config.version_tag_prefix.as_str(),
            ),
        ];
        for (k, v) in sets {
            run_git(&wd, &["config", k, v])?;
        }
        // Create the develop branch from master if it does not exist yet.
        let exists = run_git_raw(&wd, &["rev-parse", "--verify", "--quiet", &config.develop])?
            .status
            .success();
        if !exists {
            run_git(&wd, &["branch", &config.develop, &config.master])?;
        }
        Ok(())
    }

    fn flow_start(&self, repo: &RepoId, kind: FlowKind, name: &str) -> Result<String> {
        let cfg = self.flow_config(repo)?;
        let wd = self.workdir(repo)?;
        let (prefix, base) = match kind {
            FlowKind::Feature => (&cfg.feature_prefix, &cfg.develop),
            FlowKind::Release => (&cfg.release_prefix, &cfg.develop),
            FlowKind::Hotfix => (&cfg.hotfix_prefix, &cfg.master),
        };
        let branch = format!("{prefix}{name}");
        run_git(&wd, &["checkout", "-b", &branch, base])?;
        Ok(branch)
    }

    fn list_submodules(&self, repo: &RepoId) -> Result<Vec<Submodule>> {
        let wd = self.workdir(repo)?;
        let urls = submodule_urls(&wd);
        let out = run_git_raw(&wd, &["submodule", "status", "--recursive"])?;
        if !out.status.success() {
            return Ok(Vec::new());
        }
        let mut subs = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if line.is_empty() {
                continue;
            }
            // `<marker><40hex> <path> (<ref>)` — marker: ' ' ok, '-' uninit,
            // '+' out-of-date, 'U' conflicts.
            let marker = line.as_bytes()[0] as char;
            let rest = &line[1..];
            let mut it = rest.split_whitespace();
            let sha = it.next().unwrap_or("").to_string();
            let Some(path) = it.next() else {
                continue;
            };
            subs.push(Submodule {
                url: urls.get(path).cloned().unwrap_or_default(),
                path: path.to_string(),
                sha,
                initialized: marker != '-',
                dirty: marker == '+' || marker == 'U',
            });
        }
        Ok(subs)
    }

    fn add_submodule(&self, repo: &RepoId, url: &str, path: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        // `-c protocol.file.allow=always` re-enables the `file` transport for
        // this one command's process tree (incl. the embedded clone). Git
        // blocks it by default since CVE-2022-39253, which breaks adding a
        // submodule from a local path — a legitimate, user-initiated action
        // here (the URL is chosen by the user). Repo-local config does not
        // propagate to the clone subprocess, so it must be passed via `-c`.
        run_git(
            &wd,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                url,
                path,
            ],
        )
        .map(|_| ())
    }

    fn init_submodules(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["submodule", "update", "--init", "--recursive"]).map(|_| ())
    }

    fn update_submodules(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["submodule", "update", "--recursive"]).map(|_| ())
    }

    fn sync_submodules(&self, repo: &RepoId) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["submodule", "sync", "--recursive"]).map(|_| ())
    }

    fn deinit_submodule(&self, repo: &RepoId, path: &str) -> Result<()> {
        let wd = self.workdir(repo)?;
        run_git(&wd, &["submodule", "deinit", "-f", path]).map(|_| ())
    }

    fn flow_finish(&self, repo: &RepoId, kind: FlowKind, name: &str) -> Result<()> {
        let cfg = self.flow_config(repo)?;
        let wd = self.workdir(repo)?;
        let merge = |target: &str, branch: &str| -> Result<()> {
            run_git(&wd, &["checkout", target])?;
            run_git(
                &wd,
                &[
                    "merge",
                    "--no-ff",
                    "--no-edit",
                    "-m",
                    &format!("Merge branch '{branch}' into {target}"),
                    branch,
                ],
            )
            .map(|_| ())
        };
        match kind {
            FlowKind::Feature => {
                let branch = format!("{}{name}", cfg.feature_prefix);
                merge(&cfg.develop, &branch)?;
                run_git(&wd, &["branch", "-d", &branch])?;
            }
            FlowKind::Release | FlowKind::Hotfix => {
                let prefix = if matches!(kind, FlowKind::Release) {
                    &cfg.release_prefix
                } else {
                    &cfg.hotfix_prefix
                };
                let branch = format!("{prefix}{name}");
                // Merge into master and tag the release/hotfix version.
                merge(&cfg.master, &branch)?;
                let tag = format!("{}{name}", cfg.version_tag_prefix);
                run_git(&wd, &["tag", &tag])?;
                // Merge into develop too, then delete the branch.
                merge(&cfg.develop, &branch)?;
                run_git(&wd, &["branch", "-d", &branch])?;
            }
        }
        Ok(())
    }
}

/// Build a `path -> url` map from a superproject's `.gitmodules`.
fn submodule_urls(workdir: &Path) -> HashMap<String, String> {
    let out = run_git_raw(
        workdir,
        &[
            "config",
            "-f",
            ".gitmodules",
            "--get-regexp",
            r"^submodule\..*",
        ],
    );
    let mut name_path: HashMap<String, String> = HashMap::new();
    let mut name_url: HashMap<String, String> = HashMap::new();
    if let Ok(out) = out {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                // "submodule.<name>.<key> <value>"
                let Some((key, value)) = line.split_once(' ') else {
                    continue;
                };
                let parts: Vec<&str> = key.splitn(3, '.').collect();
                if parts.len() != 3 {
                    continue;
                }
                let (name, field) = (parts[1].to_string(), parts[2]);
                match field {
                    "path" => {
                        name_path.insert(name, value.trim().to_string());
                    }
                    "url" => {
                        name_url.insert(name, value.trim().to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    name_path
        .into_iter()
        .filter_map(|(name, path)| name_url.get(&name).map(|url| (path, url.clone())))
        .collect()
}

/// Read a single git config value, or `None` when unset.
fn git_config_get(workdir: &Path, key: &str) -> Option<String> {
    let out = run_git_raw(workdir, &["config", "--get", key]).ok()?;
    if !out.status.success() {
        return None;
    }
    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!val.is_empty()).then_some(val)
}

/// Read a config value from the repo's local scope only (`.git/config`), or
/// `None` when unset there. Used for per-repo identity so an inherited
/// global/system value is shown as "not overridden".
fn git_config_get_local(workdir: &Path, key: &str) -> Option<String> {
    let out = run_git_raw(workdir, &["config", "--local", "--get", key]).ok()?;
    if !out.status.success() {
        return None;
    }
    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!val.is_empty()).then_some(val)
}

/// Set (or, when `value` is blank, unset) a local config `key`. A missing key on
/// unset is not an error (git exits 5).
fn set_or_unset_local(workdir: &Path, key: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        let out = run_git_raw(workdir, &["config", "--local", "--unset", key])?;
        if !out.status.success() && out.status.code() != Some(5) {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(Error::Git(stderr.trim().to_string()));
        }
        Ok(())
    } else {
        run_git(workdir, &["config", "--local", key, value]).map(|_| ())
    }
}

/// Pull an integer out of `text` immediately following `marker` (skipping any
/// non-digits in between).
fn num_after(text: &str, marker: &str) -> Option<usize> {
    let idx = text.find(marker)?;
    let rest = &text[idx + marker.len()..];
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Parse `git worktree list --porcelain` into [`Worktree`]s. Blocks are
/// separated by blank lines; each starts with `worktree <path>`.
fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut current: Option<Worktree> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(wt) = current.take() {
                out.push(wt);
            }
            current = Some(Worktree {
                path: path.to_string(),
                display_name: String::new(),
                branch: None,
                head: None,
                is_main: false,
                selected: false,
                dirty: false,
                locked: false,
                prunable: false,
                missing: false,
            });
        } else if let Some(wt) = current.as_mut() {
            if let Some(sha) = line.strip_prefix("HEAD ") {
                wt.head = Some(Oid::from(sha.to_string()));
            } else if let Some(refname) = line.strip_prefix("branch ") {
                // `branch refs/heads/<name>` → short name.
                wt.branch = Some(
                    refname
                        .strip_prefix("refs/heads/")
                        .unwrap_or(refname)
                        .to_string(),
                );
            } else if line == "locked" || line.starts_with("locked ") {
                wt.locked = true;
            } else if line == "prunable" || line.starts_with("prunable ") {
                wt.prunable = true;
            }
        }
    }
    if let Some(wt) = current.take() {
        out.push(wt);
    }
    out
}

fn path_identity(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn display_base(wt: &Worktree) -> String {
    wt.branch
        .clone()
        .or_else(|| {
            Path::new(&wt.path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| wt.path.clone())
}

fn worktree_dirty(path: &Path) -> bool {
    run_git_raw(path, &["status", "--porcelain=v1", "-z"])
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false)
}

fn enrich_worktrees(mut worktrees: Vec<Worktree>, selected_workdir: &Path) -> Vec<Worktree> {
    let selected = path_identity(selected_workdir);

    for (index, wt) in worktrees.iter_mut().enumerate() {
        let path = Path::new(&wt.path);
        wt.is_main = index == 0;
        wt.missing = !path.exists();
        wt.selected = path_identity(path) == selected;
        wt.dirty = !wt.missing && !wt.prunable && worktree_dirty(path);
    }

    let mut counts: HashMap<String, usize> = HashMap::new();
    for wt in &worktrees {
        let base = if wt.is_main {
            "main".to_string()
        } else {
            display_base(wt)
        };
        *counts.entry(base).or_insert(0) += 1;
    }

    let mut seen: HashMap<String, usize> = HashMap::new();
    for wt in &mut worktrees {
        let base = if wt.is_main {
            "main".to_string()
        } else {
            display_base(wt)
        };
        let index = seen.entry(base.clone()).or_insert(0);
        *index += 1;
        wt.display_name = if counts.get(&base).copied().unwrap_or(0) > 1 {
            format!("{base} {}", *index)
        } else {
            base
        };
    }

    worktrees
}

// ── Status parsing ────────────────────────────────────────────────────────────

/// Map a porcelain-v2 status code char to a [`ChangeKind`]. `.` (unmodified)
/// returns `None`.
fn change_kind(code: char) -> Option<ChangeKind> {
    match code {
        'M' | 'T' => Some(ChangeKind::Modified),
        'A' => Some(ChangeKind::Added),
        'D' => Some(ChangeKind::Deleted),
        'R' | 'C' => Some(ChangeKind::Renamed),
        'U' => Some(ChangeKind::Conflicted),
        _ => None,
    }
}

/// Parse `git status --porcelain=v2 -z` output into a [`WorkingTree`].
///
/// Records are NUL-separated. A rename entry (`2 …`) is followed by a second
/// NUL-delimited token holding the original path, so it consumes two tokens.
fn parse_status_v2(bytes: &[u8]) -> WorkingTree {
    let mut staged: Vec<FileStatus> = Vec::new();
    let mut unstaged: Vec<FileStatus> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();

    let tokens: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        if tok.is_empty() {
            i += 1;
            continue;
        }
        let line = String::from_utf8_lossy(tok).into_owned();
        let bytes0 = line.as_bytes();
        match bytes0[0] {
            // Header line (`# branch.oid …`): ignored.
            b'#' => {}
            // Ordinary or rename/copy changed entry. Format:
            //   1 <XY> … <path>           |   2 <XY> … <Xscore> <path>\0<origPath>
            b'1' | b'2' => {
                let x = bytes0[2] as char;
                let y = bytes0[3] as char;
                let is_rename = bytes0[0] == b'2';
                // Path is the final space-delimited field (no quoting under -z).
                let nfields = if is_rename { 10 } else { 9 };
                let path = line.splitn(nfields, ' ').last().unwrap_or("").to_string();
                // For a rename, the original path is the next NUL-delimited token.
                let orig = if is_rename {
                    i += 1;
                    tokens
                        .get(i)
                        .map(|t| String::from_utf8_lossy(t).into_owned())
                } else {
                    None
                };
                if let Some(kind) = change_kind(x) {
                    staged.push(FileStatus {
                        path: path.clone(),
                        old_path: if x == 'R' || x == 'C' {
                            orig.clone()
                        } else {
                            None
                        },
                        kind,
                    });
                }
                if let Some(kind) = change_kind(y) {
                    unstaged.push(FileStatus {
                        path,
                        old_path: if y == 'R' || y == 'C' { orig } else { None },
                        kind,
                    });
                }
            }
            // Unmerged (conflict) entry: `u <xy> … <path>`.
            b'u' => {
                let path = line.splitn(11, ' ').last().unwrap_or("").to_string();
                unstaged.push(FileStatus {
                    path,
                    old_path: None,
                    kind: ChangeKind::Conflicted,
                });
            }
            // Untracked: `? <path>`.
            b'?' if line.len() > 2 => {
                untracked.push(line[2..].to_string());
            }
            // Ignored (`! …`) or anything unexpected: skip.
            _ => {}
        }
        i += 1;
    }

    staged.sort_by(|a, b| a.path.cmp(&b.path));
    unstaged.sort_by(|a, b| a.path.cmp(&b.path));
    untracked.sort();
    WorkingTree {
        staged,
        unstaged,
        untracked,
    }
}

// ── Tree helpers ──────────────────────────────────────────────────────────────

/// Diff two trees (`old_tree_id` = `None` ⇒ a root commit, so everything is
/// added) into a path-sorted [`FileDiff`] list. Shared by `diff_commit` (commit
/// vs its parent) and `diff_range` (base vs head across a span of commits).
fn diff_trees(
    repo: &gix::Repository,
    old_tree_id: Option<gix::ObjectId>,
    new_tree_id: gix::ObjectId,
) -> Result<Vec<FileDiff>> {
    use std::collections::HashMap;
    let mut old_blobs: HashMap<String, gix::ObjectId> = HashMap::new();
    let mut new_blobs: HashMap<String, gix::ObjectId> = HashMap::new();
    if let Some(old_id) = old_tree_id {
        collect_tree_blobs(repo, old_id, String::new(), &mut old_blobs)?;
    }
    collect_tree_blobs(repo, new_tree_id, String::new(), &mut new_blobs)?;

    let mut diffs: Vec<FileDiff> = Vec::new();
    let push = |diffs: &mut Vec<FileDiff>, path: &str, bd: BlobDiff| {
        diffs.push(FileDiff {
            path: path.to_string(),
            old_path: None,
            kind: bd.kind,
            hunks: bd.hunks,
            old_image_b64: bd.old_image_b64,
            new_image_b64: bd.new_image_b64,
        });
    };

    // Added files (in new, not old).
    for (path, new_id) in &new_blobs {
        if !old_blobs.contains_key(path) {
            push(
                &mut diffs,
                path,
                blob_diff(repo, None, Some(*new_id), path)?,
            );
        }
    }
    // Deleted files (in old, not new).
    for (path, old_id) in &old_blobs {
        if !new_blobs.contains_key(path) {
            push(
                &mut diffs,
                path,
                blob_diff(repo, Some(*old_id), None, path)?,
            );
        }
    }
    // Modified files (in both, different OID).
    for (path, new_id) in &new_blobs {
        if let Some(old_id) = old_blobs.get(path) {
            if old_id != new_id {
                push(
                    &mut diffs,
                    path,
                    blob_diff(repo, Some(*old_id), Some(*new_id), path)?,
                );
            }
        }
    }

    diffs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(diffs)
}

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

/// Read a blob's bytes by revision-qualified path (e.g. `":path"` for the
/// index, `"HEAD:path"` for the tip). Returns `None` when the path is absent
/// on that side (cat-file exits non-zero).
fn cat_blob(workdir: &Path, rev_path: &str) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .current_dir(workdir)
        .args(["cat-file", "blob", rev_path])
        .output()
        .ok()?;
    if out.status.success() {
        Some(out.stdout)
    } else {
        None
    }
}

/// Build a single-file [`FileDiff`] from two byte sides (`None` = absent).
/// Returns an empty vec when the file is absent on both sides.
fn single_file_diff(old: Option<&[u8]>, new: Option<&[u8]>, path: &str) -> Vec<FileDiff> {
    // Absent on both sides, or byte-identical: no change to show.
    if old.is_none() && new.is_none() {
        return Vec::new();
    }
    if old == new {
        return Vec::new();
    }
    let bd = blob_diff_bytes(old, new, path);
    vec![FileDiff {
        path: path.to_string(),
        old_path: None,
        kind: bd.kind,
        hunks: bd.hunks,
        old_image_b64: bd.old_image_b64,
        new_image_b64: bd.new_image_b64,
    }]
}

/// Determine `FileDiffKind` and compute text hunks (or image b64) for a
/// pair of optional blob OIDs.
fn blob_diff(
    repo: &gix::Repository,
    old_id: Option<gix::ObjectId>,
    new_id: Option<gix::ObjectId>,
    path: &str,
) -> Result<BlobDiff> {
    let old_bytes = old_id
        .map(|id| {
            repo.find_object(id)
                .map_err(backend)
                .map(|o| o.data.to_vec())
        })
        .transpose()?;
    let new_bytes = new_id
        .map(|id| {
            repo.find_object(id)
                .map_err(backend)
                .map(|o| o.data.to_vec())
        })
        .transpose()?;
    Ok(blob_diff_bytes(
        old_bytes.as_deref(),
        new_bytes.as_deref(),
        path,
    ))
}

/// Diff one file from raw byte sides. `None` means the file is absent on that
/// side (so old=None → Added, new=None → Deleted). Detects image (by
/// extension) and binary (null bytes) content; otherwise produces text hunks.
fn blob_diff_bytes(old: Option<&[u8]>, new: Option<&[u8]>, path: &str) -> BlobDiff {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use lady_diff::text_diff;

    // Detect image by extension.
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "tiff" | "webp" | "svg"
    ) {
        let kind = match (old, new) {
            (None, _) => FileDiffKind::Added,
            (_, None) => FileDiffKind::Deleted,
            _ => FileDiffKind::Image,
        };
        return BlobDiff {
            kind,
            hunks: Vec::new(),
            old_image_b64: old.map(|b| B64.encode(b)),
            new_image_b64: new.map(|b| B64.encode(b)),
        };
    }

    // Binary detection: look for null bytes on either side.
    let is_binary = old.is_some_and(|b| b.contains(&0)) || new.is_some_and(|b| b.contains(&0));
    if is_binary {
        let kind = match (old, new) {
            (None, _) => FileDiffKind::Added,
            (_, None) => FileDiffKind::Deleted,
            _ => FileDiffKind::Binary,
        };
        return BlobDiff {
            kind,
            hunks: Vec::new(),
            old_image_b64: None,
            new_image_b64: None,
        };
    }

    let old_text = String::from_utf8_lossy(old.unwrap_or(&[]));
    let new_text = String::from_utf8_lossy(new.unwrap_or(&[]));
    let kind = match (old, new) {
        (None, _) => FileDiffKind::Added,
        (_, None) => FileDiffKind::Deleted,
        _ => FileDiffKind::Modified,
    };
    BlobDiff {
        kind,
        hunks: text_diff(&old_text, &new_text),
        old_image_b64: None,
        new_image_b64: None,
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
    fn git_auth_none_leaves_invocation_unchanged() {
        // The default path must not perturb the command: an empty GitAuth adds no
        // `-c` flags and no env, so a plain `status` still succeeds.
        let dir = fixture_repo();
        let mut sink = |_: &str| {};
        run_git_streaming(
            dir.path(),
            &["status", "--porcelain"],
            &GitAuth::none(),
            &mut sink,
        )
        .expect("status with no auth overrides");
    }

    #[test]
    fn git_auth_applies_config_flags() {
        // Inject a config value git will reject, proving the `-c key=value` flag
        // actually reached the child process. git's verbatim error is captured.
        let dir = fixture_repo();
        let auth = GitAuth {
            config: vec![("core.bare".to_string(), "notabool".to_string())],
            env: Vec::new(),
        };
        let mut sink = |_: &str| {};
        let err = run_git_streaming(dir.path(), &["status"], &auth, &mut sink)
            .expect_err("a bad injected -c value must fail");
        assert!(
            format!("{err}").contains("notabool"),
            "git should reject the injected config, proving it was applied: {err}"
        );
    }

    #[test]
    fn git_auth_applies_env() {
        // Point GIT_TRACE at a file via the env channel; git writing to it proves
        // the env var reached the child (the same channel SSH-key overrides use).
        let dir = fixture_repo();
        let trace = dir.path().join("trace.log");
        let auth = GitAuth {
            config: Vec::new(),
            env: vec![(
                "GIT_TRACE".to_string(),
                trace.to_string_lossy().into_owned(),
            )],
        };
        let mut sink = |_: &str| {};
        run_git_streaming(dir.path(), &["status", "--porcelain"], &auth, &mut sink)
            .expect("status with GIT_TRACE env");
        let logged = std::fs::read_to_string(&trace).unwrap_or_default();
        assert!(!logged.is_empty(), "GIT_TRACE env should have been applied");
    }

    #[test]
    fn repo_identity_round_trips_local_config() {
        let dir = fixture_repo();
        let engine = GixEngine::new();
        let id = engine.open(dir.path()).expect("open the fixture repo");

        engine
            .repo_identity_set(&id, "Ada Lovelace", "ada@example.com")
            .expect("set identity");
        let got = engine.repo_identity_get(&id).expect("get identity");
        assert_eq!(got.name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(got.email.as_deref(), Some("ada@example.com"));

        // An empty value unsets the key (so it inherits the global/system value).
        engine
            .repo_identity_set(&id, "", "keep@example.com")
            .expect("unset the name");
        let got = engine.repo_identity_get(&id).expect("get identity again");
        assert_eq!(got.name, None, "blank name should unset user.name locally");
        assert_eq!(got.email.as_deref(), Some("keep@example.com"));
    }

    #[test]
    fn diff_range_spans_commits_and_matches_diff_commit_for_one() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");
        let rev = |r: &str| {
            String::from_utf8_lossy(&run_git(p, &["rev-parse", r]).expect("rev-parse").stdout)
                .trim()
                .to_string()
        };
        // fixture has commits adding file1..file3.
        let head = Oid::from(rev("HEAD"));
        let base2 = Oid::from(rev("HEAD~2")); // commit 1
        let parent3 = Oid::from(rev("HEAD~1")); // commit 2

        // Span commit2..commit3 == diff_commit(commit3) (both = file3 added).
        let span = engine
            .diff_range(&id, &parent3, &head)
            .expect("diff_range one");
        let single = engine.diff_commit(&id, &head).expect("diff_commit");
        let names = |v: &[FileDiff]| {
            let mut n: Vec<String> = v.iter().map(|f| f.path.clone()).collect();
            n.sort();
            n
        };
        assert_eq!(names(&span), names(&single));
        assert_eq!(names(&span), vec!["file3.txt".to_string()]);

        // Span commit1..commit3 covers file2 + file3.
        let wide = engine
            .diff_range(&id, &base2, &head)
            .expect("diff_range wide");
        assert_eq!(
            names(&wide),
            vec!["file2.txt".to_string(), "file3.txt".to_string()]
        );
    }

    #[test]
    fn reset_soft_mixed_then_hard_round_trips() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");
        let rev = |r: &str| {
            String::from_utf8_lossy(&run_git(p, &["rev-parse", r]).expect("rev-parse").stdout)
                .trim()
                .to_string()
        };
        let tip = Oid::from(rev("HEAD"));
        let base = Oid::from(rev("HEAD~2"));

        // Soft reset to base: HEAD moves back, but the index keeps the span's
        // content, so the difference shows up as *staged* changes only.
        engine
            .reset(&id, &base, ResetMode::Soft)
            .expect("soft reset");
        assert_eq!(
            engine.head_commit(&id).expect("head").as_str(),
            base.as_str()
        );
        let staged = engine.status(&id).expect("status");
        assert!(
            !staged.staged.is_empty(),
            "soft reset should leave the span's changes staged"
        );
        assert!(
            staged.unstaged.is_empty(),
            "soft reset should not leave unstaged changes"
        );

        // Mixed reset to base: HEAD moves back, the span's files remain on disk
        // (now untracked/modified), so the tree is dirty.
        engine
            .reset(&id, &base, ResetMode::Mixed)
            .expect("mixed reset");
        assert_eq!(
            engine.head_commit(&id).expect("head").as_str(),
            base.as_str()
        );
        let dirty = engine.status(&id).expect("status");
        assert!(
            !dirty.staged.is_empty() || !dirty.unstaged.is_empty() || !dirty.untracked.is_empty(),
            "mixed reset should leave the span's changes in the working tree"
        );

        // Hard reset back to the tip restores the original clean state.
        engine
            .reset(&id, &tip, ResetMode::Hard)
            .expect("hard reset");
        assert_eq!(
            engine.head_commit(&id).expect("head").as_str(),
            tip.as_str()
        );
        let clean = engine.status(&id).expect("status");
        assert!(
            clean.staged.is_empty() && clean.unstaged.is_empty() && clean.untracked.is_empty(),
            "hard reset to the tip should restore a clean tree"
        );
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
        // The Head ref is named after the checked-out branch (here, `main`).
        let head = named(RefKind::Head, "main");

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

    /// Same shape as the `repo_ops` bench: a 512-commit synthetic repo must walk
    /// cleanly via gix (guards against CI bench flakes).
    #[test]
    fn walk_log_synthetic_512_commit_repo() {
        let Some(repo) = lady_fixtures::build_synthetic_repo(512, 0xC0FFEE) else {
            return; // no system git in this environment
        };
        let engine = GixEngine::new();
        let id = engine.open(repo.path()).expect("open synthetic repo");
        let commits = engine
            .walk_log(
                &id,
                GraphQuery {
                    start: None,
                    limit: 512,
                },
            )
            .expect("walk_log on 512-commit synthetic repo");
        assert_eq!(commits.len(), 512);
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

    #[test]
    fn is_dirty_reflects_worktree_changes() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        // Fresh checkout from a committed fixture: clean.
        assert!(!engine.is_dirty(&id).expect("status on clean tree"));

        // Modify a tracked file → dirty.
        let tracked = std::fs::read_dir(p)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|pp| pp.extension().is_some_and(|x| x == "txt"))
            .expect("a tracked .txt file exists");
        std::fs::write(&tracked, "mutated\n").expect("write");
        assert!(engine.is_dirty(&id).expect("status on dirty tree"));
    }

    #[test]
    fn list_files_returns_sorted_tracked_paths() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        std::fs::write(p.join("z.txt"), "z\n").expect("write");
        std::fs::create_dir(p.join("src")).expect("mkdir");
        std::fs::write(p.join("src/main.rs"), "fn main() {}\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "files"]);

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");
        let files = engine.list_files(&id).expect("list_files");

        assert_eq!(files, vec!["src/main.rs".to_owned(), "z.txt".to_owned()]);
    }

    #[test]
    fn status_buckets_staged_unstaged_and_untracked() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        // Baseline commit with one tracked file.
        std::fs::write(p.join("tracked.txt"), "base\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "base"]);

        // Staged: a brand-new file, added to the index.
        std::fs::write(p.join("staged.txt"), "new\n").expect("write");
        git(p, &["add", "staged.txt"]);
        // Unstaged: modify the tracked file on disk without staging it.
        std::fs::write(p.join("tracked.txt"), "modified\n").expect("write");
        // Untracked: a file git has never seen.
        std::fs::write(p.join("untracked.txt"), "scratch\n").expect("write");

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");
        let wt = engine.status(&id).expect("status on a dirty tree");

        assert_eq!(wt.staged.len(), 1, "one staged change: {:?}", wt.staged);
        assert_eq!(wt.staged[0].path, "staged.txt");
        assert_eq!(wt.staged[0].kind, ChangeKind::Added);

        assert_eq!(
            wt.unstaged.len(),
            1,
            "one unstaged change: {:?}",
            wt.unstaged
        );
        assert_eq!(wt.unstaged[0].path, "tracked.txt");
        assert_eq!(wt.unstaged[0].kind, ChangeKind::Modified);

        assert_eq!(wt.untracked, vec!["untracked.txt".to_owned()]);
    }

    #[test]
    fn status_clean_tree_is_empty() {
        let dir = fixture_repo();
        let engine = GixEngine::new();
        let id = engine.open(dir.path()).expect("open the fixture repo");
        let wt = engine.status(&id).expect("status on a clean tree");
        assert!(wt.staged.is_empty() && wt.unstaged.is_empty() && wt.untracked.is_empty());
    }

    #[test]
    fn status_detects_a_staged_rename() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        std::fs::write(p.join("old.txt"), "some content here\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "base"]);

        // Rename via git so the move is staged as a rename.
        git(p, &["mv", "old.txt", "new.txt"]);

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");
        let wt = engine.status(&id).expect("status after rename");

        let renamed = wt
            .staged
            .iter()
            .find(|f| f.kind == ChangeKind::Renamed)
            .expect("a staged rename entry");
        assert_eq!(renamed.path, "new.txt");
        assert_eq!(renamed.old_path.as_deref(), Some("old.txt"));
    }

    #[test]
    fn stage_then_unstage_moves_a_path_between_buckets() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        std::fs::write(p.join("a.txt"), "base\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "base"]);

        // Modify on disk — starts unstaged.
        std::fs::write(p.join("a.txt"), "changed\n").expect("write");

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");
        let paths = vec!["a.txt".to_string()];

        assert_eq!(engine.status(&id).unwrap().unstaged.len(), 1);

        engine.stage_paths(&id, &paths).expect("stage");
        let wt = engine.status(&id).unwrap();
        assert_eq!(wt.staged.len(), 1, "now staged");
        assert!(wt.unstaged.is_empty(), "no longer unstaged");

        engine.unstage_paths(&id, &paths).expect("unstage");
        let wt = engine.status(&id).unwrap();
        assert!(wt.staged.is_empty(), "no longer staged");
        assert_eq!(wt.unstaged.len(), 1, "unstaged again");
    }

    #[test]
    fn diff_spec_covers_commit_working_and_staged() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        std::fs::write(p.join("a.txt"), "one\ntwo\nthree\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "base"]);

        // Stage a change to line 1, then make a further unstaged change to line 3.
        std::fs::write(p.join("a.txt"), "ONE\ntwo\nthree\n").expect("write");
        git(p, &["add", "a.txt"]);
        std::fs::write(p.join("a.txt"), "ONE\ntwo\nTHREE\n").expect("write");

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // Staged: HEAD ("one") vs index ("ONE") — sees the line-1 change only.
        let staged = engine
            .diff_spec(&id, &DiffSpec::IndexVsHead("a.txt".to_string()))
            .expect("staged diff");
        assert_eq!(staged.len(), 1);
        let staged_added: Vec<&str> = staged[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == lady_proto::LineKind::Added)
            .map(|l| l.content.as_str())
            .collect();
        assert!(staged_added.contains(&"ONE"), "staged diff adds ONE");
        assert!(!staged_added.contains(&"THREE"), "staged diff omits THREE");

        // Unstaged: index ("ONE..three") vs working ("ONE..THREE") — line-3 only.
        let working = engine
            .diff_spec(&id, &DiffSpec::WorkingVsIndex("a.txt".to_string()))
            .expect("working diff");
        let working_added: Vec<&str> = working[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == lady_proto::LineKind::Added)
            .map(|l| l.content.as_str())
            .collect();
        assert!(working_added.contains(&"THREE"), "working diff adds THREE");
        assert!(!working_added.contains(&"ONE"), "working diff omits ONE");

        // Commit variant still works (root commit adds a.txt).
        let head = engine.list_refs(&id).unwrap();
        let head_oid = head
            .iter()
            .find(|r| r.kind == RefKind::Head)
            .unwrap()
            .target
            .clone();
        let commit_diff = engine
            .diff_spec(&id, &DiffSpec::Commit(head_oid))
            .expect("commit diff");
        assert_eq!(commit_diff.len(), 1);
        assert_eq!(commit_diff[0].path, "a.txt");
    }

    #[test]
    fn apply_patch_stages_one_hunk_of_two() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        // 40-line base; change line 5 and line 35 → two well-separated hunks.
        let base: String = (1..=40).map(|i| format!("line{i}\n")).collect();
        std::fs::write(p.join("f.txt"), &base).expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "base"]);

        let modified: String = (1..=40)
            .map(|i| match i {
                5 => "FIRST\n".to_owned(),
                35 => "SECOND\n".to_owned(),
                _ => format!("line{i}\n"),
            })
            .collect();
        std::fs::write(p.join("f.txt"), &modified).expect("write");

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // The unstaged (working-vs-index) diff has two hunks.
        let working = engine
            .diff_spec(&id, &DiffSpec::WorkingVsIndex("f.txt".to_string()))
            .expect("working diff");
        assert_eq!(working[0].hunks.len(), 2, "two hunks before staging");

        // Build a patch for only the first hunk and stage it.
        let patch = lady_diff::build_patch("f.txt", &working[0].hunks, &[0]);
        engine
            .apply_patch(&id, &patch, false, true)
            .expect("git apply --cached must accept the patch");

        // Staged diff (index-vs-HEAD) must contain only the FIRST change.
        let staged = engine
            .diff_spec(&id, &DiffSpec::IndexVsHead("f.txt".to_string()))
            .expect("staged diff");
        let staged_added: Vec<&str> = staged[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == lady_proto::LineKind::Added)
            .map(|l| l.content.as_str())
            .collect();
        assert_eq!(staged_added, vec!["FIRST"], "only the first hunk is staged");

        // The SECOND change remains unstaged (still in working-vs-index).
        let working_after = engine
            .diff_spec(&id, &DiffSpec::WorkingVsIndex("f.txt".to_string()))
            .expect("working diff after");
        let working_added: Vec<&str> = working_after[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == lady_proto::LineKind::Added)
            .map(|l| l.content.as_str())
            .collect();
        assert_eq!(
            working_added,
            vec!["SECOND"],
            "the second hunk stays unstaged"
        );

        // Index is not corrupted: status parses and lists f.txt in both sets.
        let wt = engine.status(&id).expect("status after partial stage");
        assert!(wt.staged.iter().any(|f| f.path == "f.txt"));
        assert!(wt.unstaged.iter().any(|f| f.path == "f.txt"));

        // Reverse the same patch (cached) to unstage it again.
        engine
            .apply_patch(&id, &patch, true, true)
            .expect("reverse apply --cached unstages");
        let staged_empty = engine
            .diff_spec(&id, &DiffSpec::IndexVsHead("f.txt".to_string()))
            .expect("staged diff after unstage");
        assert!(staged_empty.is_empty(), "nothing staged after reverse");
    }

    #[test]
    fn line_level_stage_and_discard_restore() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);
        git(p, &["config", "commit.gpgsign", "false"]);

        std::fs::write(p.join("f.txt"), "base\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "base"]);

        // Add two new lines after base.
        std::fs::write(p.join("f.txt"), "base\nADD1\nADD2\n").expect("write");

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let working = engine
            .diff_spec(&id, &DiffSpec::WorkingVsIndex("f.txt".to_string()))
            .expect("working diff");
        let hunks = &working[0].hunks;
        let added: Vec<usize> = hunks[0]
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind == lady_proto::LineKind::Added)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(added.len(), 2);

        // Stage only the first added line.
        let sel = vec![lady_diff::LineSel {
            hunk: 0,
            lines: vec![added[0]],
        }];
        let patch = lady_diff::build_patch_lines("f.txt", hunks, &sel);
        engine
            .apply_patch(&id, &patch, false, true)
            .expect("stage one line");

        let staged = engine
            .diff_spec(&id, &DiffSpec::IndexVsHead("f.txt".to_string()))
            .expect("staged diff");
        let staged_added: Vec<&str> = staged[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.kind == lady_proto::LineKind::Added)
            .map(|l| l.content.as_str())
            .collect();
        assert_eq!(staged_added, vec!["ADD1"], "only ADD1 staged");

        // Discard ADD2 from the working tree (reverse, non-cached).
        // Rebuild the working diff (it changed after staging ADD1).
        let working2 = engine
            .diff_spec(&id, &DiffSpec::WorkingVsIndex("f.txt".to_string()))
            .expect("working diff 2");
        let h2 = &working2[0].hunks;
        let add2_idx: Vec<usize> = h2[0]
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.kind == lady_proto::LineKind::Added)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(add2_idx.len(), 1, "only ADD2 remains unstaged");
        let dpatch = lady_diff::build_patch_lines(
            "f.txt",
            h2,
            &[lady_diff::LineSel {
                hunk: 0,
                lines: vec![add2_idx[0]],
            }],
        );
        engine
            .apply_patch(&id, &dpatch, true, false)
            .expect("discard ADD2 from working tree");

        let on_disk = std::fs::read_to_string(p.join("f.txt")).expect("read");
        assert!(
            !on_disk.contains("ADD2"),
            "ADD2 discarded from working tree"
        );
        assert!(on_disk.contains("ADD1"), "ADD1 (staged) still present");
    }

    #[test]
    fn discard_untracked_removes_files() {
        let dir = fixture_repo();
        let p = dir.path();
        std::fs::write(p.join("junk.tmp"), "x\n").expect("write");

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");
        assert!(engine
            .status(&id)
            .unwrap()
            .untracked
            .contains(&"junk.tmp".to_string()));

        engine
            .discard_untracked(&id, &["junk.tmp".to_string()])
            .expect("discard untracked");
        assert!(!p.join("junk.tmp").exists(), "file deleted");
        assert!(engine.status(&id).unwrap().untracked.is_empty());
    }

    #[test]
    fn stage_and_unstage_handle_unborn_branch() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.name", "Lady Test"]);
        git(p, &["config", "user.email", "test@example.com"]);

        // No commits yet (unborn HEAD); a brand-new untracked file.
        std::fs::write(p.join("first.txt"), "hi\n").expect("write");

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");
        let paths = vec!["first.txt".to_string()];

        assert_eq!(engine.status(&id).unwrap().untracked, vec!["first.txt"]);

        engine.stage_paths(&id, &paths).expect("stage on unborn");
        let wt = engine.status(&id).unwrap();
        assert_eq!(wt.staged.len(), 1, "staged Added on unborn branch");
        assert_eq!(wt.staged[0].kind, ChangeKind::Added);

        engine
            .unstage_paths(&id, &paths)
            .expect("unstage on unborn");
        let wt = engine.status(&id).unwrap();
        assert!(wt.staged.is_empty(), "unstaged back to untracked");
        assert_eq!(wt.untracked, vec!["first.txt"]);
    }

    #[test]
    fn commit_records_staged_then_amend_rewrites_tip() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let count = |p: &Path| -> usize {
            let out = std::process::Command::new("git")
                .current_dir(p)
                .args(["rev-list", "--count", "HEAD"])
                .output()
                .expect("rev-list");
            String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
        };
        let before = count(p);

        // Stage a new file and commit it.
        std::fs::write(p.join("new.txt"), "fresh\n").expect("write");
        engine
            .stage_paths(&id, &["new.txt".to_string()])
            .expect("stage");
        let oid = engine
            .commit(&id, "add new.txt", &CommitOpts::default())
            .expect("commit");

        assert_eq!(count(p), before + 1, "commit adds exactly one commit");
        // The returned Oid is the new tip and its diff contains the staged file.
        let diff = engine
            .diff_spec(&id, &DiffSpec::Commit(oid.clone()))
            .unwrap();
        assert!(
            diff.iter().any(|f| f.path == "new.txt"),
            "tip commit diff includes the committed file"
        );
        let msgs = engine.recent_messages(&id, 5).expect("recent");
        assert_eq!(msgs.first().map(String::as_str), Some("add new.txt"));

        // Amend: stage one more change, rewrite the tip in place.
        std::fs::write(p.join("new.txt"), "fresh\nmore\n").expect("write");
        engine
            .stage_paths(&id, &["new.txt".to_string()])
            .expect("stage");
        let amended = engine
            .commit(
                &id,
                "add new.txt (amended)",
                &CommitOpts {
                    amend: true,
                    sign: false,
                },
            )
            .expect("amend");

        assert_eq!(count(p), before + 1, "amend does NOT add a commit");
        assert_ne!(amended, oid, "amend rewrites the tip to a new Oid");
        let msgs = engine.recent_messages(&id, 5).expect("recent");
        assert_eq!(
            msgs.first().map(String::as_str),
            Some("add new.txt (amended)"),
            "amend replaced the tip message"
        );
    }

    fn current_branch(p: &Path) -> String {
        let out = std::process::Command::new("git")
            .current_dir(p)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .expect("rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn current_upstream(p: &Path) -> String {
        let out = std::process::Command::new("git")
            .current_dir(p)
            .args(["rev-parse", "--abbrev-ref", "@{u}"])
            .output()
            .expect("rev-parse upstream");
        assert!(out.status.success(), "current branch should have upstream");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn create_checkout_and_delete_branch() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");
        let start = current_branch(p);

        engine.create_branch(&id, "feature", None).expect("create");
        engine.checkout(&id, "feature", false).expect("checkout");
        assert_eq!(current_branch(p), "feature", "HEAD moved to new branch");

        // Can't delete the branch we're on; switch back first, then delete.
        engine.checkout(&id, &start, false).expect("checkout back");
        engine.delete_branch(&id, "feature", false).expect("delete");
        let refs = engine.list_refs(&id).expect("refs");
        assert!(
            !refs.iter().any(|r| r.name == "feature"),
            "branch ref removed"
        );
    }

    #[test]
    fn checkout_remote_tracking_creates_local_tracking_branch() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();

        let remote = root.join("remote.git");
        git(root, &["init", "-q", "--bare", "-b", "main", "remote.git"]);

        let seed = root.join("seed");
        git(
            root,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                seed.to_str().unwrap(),
            ],
        );
        git(&seed, &["config", "user.name", "Lady Test"]);
        git(&seed, &["config", "user.email", "test@example.com"]);
        git(&seed, &["config", "commit.gpgsign", "false"]);
        std::fs::write(seed.join("file.txt"), "main\n").expect("write");
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-q", "-m", "main"]);
        git(&seed, &["push", "-q", "origin", "main"]);
        git(&seed, &["checkout", "-q", "-b", "feature/deep"]);
        std::fs::write(seed.join("feature.txt"), "feature\n").expect("write");
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-q", "-m", "feature"]);
        git(&seed, &["push", "-q", "origin", "feature/deep"]);

        let clone = root.join("clone");
        git(
            root,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );

        let engine = GixEngine::new();
        let id = engine.open(&clone).expect("open clone");
        engine
            .checkout(&id, "origin/feature/deep", false)
            .expect("checkout remote tracking branch");

        assert_eq!(current_branch(&clone), "feature/deep");
        assert_eq!(current_upstream(&clone), "origin/feature/deep");
    }

    #[test]
    fn create_then_delete_tag() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        engine
            .create_tag(&id, "v1.0", None, Some("release one"))
            .expect("create tag");
        let refs = engine.list_refs(&id).expect("refs");
        assert!(
            refs.iter().any(|r| r.name == "v1.0"),
            "annotated tag ref present"
        );

        engine.delete_tag(&id, "v1.0").expect("delete tag");
        let refs = engine.list_refs(&id).expect("refs");
        assert!(!refs.iter().any(|r| r.name == "v1.0"), "tag ref removed");
    }

    #[test]
    fn checkout_refuses_to_clobber_local_changes() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // On `other`, change file1.txt so the two branches diverge there.
        engine.create_branch(&id, "other", None).expect("create");
        engine
            .checkout(&id, "other", false)
            .expect("checkout other");
        std::fs::write(p.join("file1.txt"), "branch side\n").expect("write");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage");
        engine
            .commit(&id, "diverge on other", &CommitOpts::default())
            .expect("commit");

        // Back on `main`, make an uncommitted edit to that same file.
        engine.checkout(&id, "main", false).expect("checkout main");
        std::fs::write(p.join("file1.txt"), "uncommitted local edit\n").expect("write");

        // Switching to `other` would overwrite the local edit → git refuses.
        let res = engine.checkout(&id, "other", false);
        assert!(
            res.is_err(),
            "checkout that clobbers local changes is refused"
        );
    }

    /// Push / fetch / ahead_behind round-trip over a local `file://`-style bare
    /// remote — fully offline, no network or credentials involved.
    #[test]
    fn push_fetch_and_ahead_behind_round_trip() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();

        // Bare "remote" that the working clones push to / fetch from.
        let remote = root.join("remote.git");
        git(root, &["init", "-q", "--bare", "-b", "main", "remote.git"]);

        // Clone A: seed an initial commit and push it so `main` exists upstream.
        let a = root.join("a");
        git(
            root,
            &["clone", "-q", remote.to_str().unwrap(), a.to_str().unwrap()],
        );
        git(&a, &["config", "user.name", "Lady Test"]);
        git(&a, &["config", "user.email", "test@example.com"]);
        git(&a, &["config", "commit.gpgsign", "false"]);
        std::fs::write(a.join("file.txt"), "v1\n").expect("write");
        git(&a, &["add", "."]);
        git(&a, &["commit", "-q", "-m", "first"]);

        let engine = GixEngine::new();
        let id_a = engine.open(&a).expect("open clone a");
        let mut sink = |_: &str| {};

        // First push sets upstream so ahead/behind has something to compare to.
        engine
            .push(
                &id_a,
                Some("origin"),
                Some("main"),
                true,
                false,
                &GitAuth::none(),
                &mut sink,
            )
            .expect("push main");
        assert_eq!(
            engine.ahead_behind(&id_a).expect("ahead_behind"),
            Some(AheadBehind {
                ahead: 0,
                behind: 0
            }),
            "in sync right after pushing"
        );

        // A local commit not yet pushed → ahead by one.
        std::fs::write(a.join("file.txt"), "v2\n").expect("write");
        git(&a, &["add", "."]);
        git(&a, &["commit", "-q", "-m", "second"]);
        assert_eq!(
            engine.ahead_behind(&id_a).expect("ahead_behind"),
            Some(AheadBehind {
                ahead: 1,
                behind: 0
            }),
            "one unpushed local commit"
        );
        engine
            .push(&id_a, None, None, false, false, &GitAuth::none(), &mut sink)
            .expect("push second");

        // Clone B pushes a third commit; A fetches it → A is now behind by one.
        let b = root.join("b");
        git(
            root,
            &["clone", "-q", remote.to_str().unwrap(), b.to_str().unwrap()],
        );
        git(&b, &["config", "user.name", "Lady Test"]);
        git(&b, &["config", "user.email", "test@example.com"]);
        git(&b, &["config", "commit.gpgsign", "false"]);
        std::fs::write(b.join("file.txt"), "v3\n").expect("write");
        git(&b, &["add", "."]);
        git(&b, &["commit", "-q", "-m", "third"]);
        let engine_b = GixEngine::new();
        let id_b = engine_b.open(&b).expect("open clone b");
        engine_b
            .push(&id_b, None, None, false, false, &GitAuth::none(), &mut sink)
            .expect("push third from b");

        engine
            .fetch(&id_a, Some("origin"), &GitAuth::none(), &mut sink)
            .expect("fetch into a");
        assert_eq!(
            engine.ahead_behind(&id_a).expect("ahead_behind"),
            Some(AheadBehind {
                ahead: 0,
                behind: 1
            }),
            "behind by the commit fetched from b"
        );
    }

    /// A branch with no upstream has no ahead/behind to report.
    #[test]
    fn ahead_behind_is_none_without_upstream() {
        let dir = fixture_repo();
        let engine = GixEngine::new();
        let id = engine.open(dir.path()).expect("open the fixture repo");
        assert_eq!(
            engine.ahead_behind(&id).expect("ahead_behind"),
            None,
            "fixture's main tracks no upstream"
        );
    }

    /// First push of a branch with no upstream must pass `origin <branch>` —
    /// `git push -u` alone fails with "no upstream branch".
    #[test]
    fn push_new_branch_sets_upstream_on_remote() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        let remote = root.join("remote.git");
        git(root, &["init", "-q", "--bare", "-b", "main", "remote.git"]);

        let a = root.join("a");
        git(
            root,
            &["clone", "-q", remote.to_str().unwrap(), a.to_str().unwrap()],
        );
        git(&a, &["config", "user.name", "Lady Test"]);
        git(&a, &["config", "user.email", "test@example.com"]);
        git(&a, &["config", "commit.gpgsign", "false"]);
        std::fs::write(a.join("file.txt"), "v1\n").expect("write");
        git(&a, &["add", "."]);
        git(&a, &["commit", "-q", "-m", "first"]);
        git(&a, &["push", "-u", "origin", "main"]);

        git(&a, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(a.join("file.txt"), "v2\n").expect("write");
        git(&a, &["add", "."]);
        git(&a, &["commit", "-q", "-m", "on feature"]);

        let engine = GixEngine::new();
        let id = engine.open(&a).expect("open clone a");
        let mut sink = |_: &str| {};

        assert_eq!(
            engine.ahead_behind(&id).expect("ahead_behind"),
            None,
            "feature has no upstream yet"
        );
        engine
            .push(&id, None, None, true, false, &GitAuth::none(), &mut sink)
            .expect("push new branch with set_upstream");
        assert_eq!(
            engine.ahead_behind(&id).expect("ahead_behind"),
            Some(AheadBehind {
                ahead: 0,
                behind: 0
            }),
            "upstream recorded after first push"
        );
    }

    #[test]
    fn stash_save_list_apply_pop_and_drop() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        // Dirty file1.txt, stash it → working tree clean again, one stash entry.
        std::fs::write(p.join("file1.txt"), "stashed edit\n").expect("write");
        engine
            .stash_save(&id, Some("WIP one"), false)
            .expect("stash save");
        assert!(
            !engine.is_dirty(&id).expect("dirty"),
            "stash cleaned the tree"
        );

        let list = engine.stash_list(&id).expect("stash list");
        assert_eq!(list.len(), 1, "one stash saved");
        assert_eq!(list[0].index, 0);
        assert!(
            list[0].message.contains("WIP one"),
            "message preserved: {}",
            list[0].message
        );

        // apply re-applies but keeps the entry.
        engine.stash_apply(&id, 0).expect("stash apply");
        assert!(
            engine.is_dirty(&id).expect("dirty"),
            "apply restored the edit"
        );
        assert_eq!(
            engine.stash_list(&id).expect("stash list").len(),
            1,
            "apply keeps the stash"
        );

        // drop removes without touching the tree; re-stash then pop to verify pop.
        engine.stash_drop(&id, 0).expect("stash drop");
        assert!(
            engine.stash_list(&id).expect("stash list").is_empty(),
            "drop removed the stash"
        );

        // Tree is still dirty from the apply; stash + pop round-trips it back.
        engine.stash_save(&id, None, false).expect("stash save 2");
        assert!(
            !engine.is_dirty(&id).expect("dirty"),
            "second stash cleaned tree"
        );
        engine.stash_pop(&id, 0).expect("stash pop");
        assert!(
            engine.is_dirty(&id).expect("dirty"),
            "pop restored the edit"
        );
        assert!(
            engine.stash_list(&id).expect("stash list").is_empty(),
            "pop removed the stash"
        );
    }

    #[test]
    fn merge_ff_only_fast_forwards() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        engine.create_branch(&id, "feature", None).expect("branch");
        engine
            .checkout(&id, "feature", false)
            .expect("checkout feature");
        std::fs::write(p.join("feature.txt"), "feature\n").expect("write");
        engine
            .stage_paths(&id, &["feature.txt".to_string()])
            .expect("stage");
        let feature_head = engine
            .commit(&id, "feature commit", &CommitOpts::default())
            .expect("commit feature");

        engine.checkout(&id, "main", false).expect("checkout main");
        let outcome = engine
            .merge(
                &id,
                "feature",
                &MergeOpts {
                    fast_forward: FfMode::Only,
                    commit_message: None,
                },
            )
            .expect("merge feature");

        assert_eq!(outcome, MergeOutcome::FastForwarded);
        assert_eq!(head_oid(p).expect("head"), feature_head);
    }

    #[test]
    fn merge_no_ff_creates_merge_commit() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        engine.create_branch(&id, "side", None).expect("branch");
        engine.checkout(&id, "side", false).expect("checkout side");
        std::fs::write(p.join("side.txt"), "side\n").expect("write side");
        engine
            .stage_paths(&id, &["side.txt".to_string()])
            .expect("stage side");
        engine
            .commit(&id, "side commit", &CommitOpts::default())
            .expect("commit side");

        engine.checkout(&id, "main", false).expect("checkout main");
        std::fs::write(p.join("main.txt"), "main\n").expect("write main");
        engine
            .stage_paths(&id, &["main.txt".to_string()])
            .expect("stage main");
        engine
            .commit(&id, "main commit", &CommitOpts::default())
            .expect("commit main");

        let outcome = engine
            .merge(
                &id,
                "side",
                &MergeOpts {
                    fast_forward: FfMode::Never,
                    commit_message: Some("merge side".to_string()),
                },
            )
            .expect("merge side");

        let MergeOutcome::Merged(merge_oid) = outcome else {
            panic!("expected merge commit, got {outcome:?}");
        };
        assert_eq!(head_oid(p).expect("head"), merge_oid);
        let parents =
            run_git(p, &["rev-list", "--parents", "-n", "1", "HEAD"]).expect("rev-list parents");
        assert_eq!(
            String::from_utf8_lossy(&parents.stdout)
                .split_whitespace()
                .skip(1)
                .count(),
            2,
            "merge commit has two parents"
        );
    }

    #[test]
    fn conflicting_merge_reports_paths_and_abort_restores_head() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        engine.create_branch(&id, "side", None).expect("branch");
        engine.checkout(&id, "side", false).expect("checkout side");
        std::fs::write(p.join("file1.txt"), "side edit\n").expect("write side");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage side");
        engine
            .commit(&id, "side edit", &CommitOpts::default())
            .expect("commit side");

        engine.checkout(&id, "main", false).expect("checkout main");
        std::fs::write(p.join("file1.txt"), "main edit\n").expect("write main");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage main");
        engine
            .commit(&id, "main edit", &CommitOpts::default())
            .expect("commit main");
        let before = head_oid(p).expect("head before");

        let outcome = engine
            .merge(&id, "side", &MergeOpts::default())
            .expect("conflicting merge should report conflicts");
        assert_eq!(
            outcome,
            MergeOutcome::Conflicts(vec!["file1.txt".to_string()])
        );

        engine.merge_abort(&id).expect("merge abort");
        assert_eq!(head_oid(p).expect("head after abort"), before);
        let wt = engine.status(&id).expect("status after abort");
        assert!(
            wt.staged.is_empty() && wt.unstaged.is_empty() && wt.untracked.is_empty(),
            "abort restores a clean tree: {wt:?}"
        );
    }

    #[test]
    fn cherry_pick_applies_commit_from_another_branch() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        engine.create_branch(&id, "side", None).expect("branch");
        engine.checkout(&id, "side", false).expect("checkout side");
        std::fs::write(p.join("side.txt"), "side\n").expect("write side");
        engine
            .stage_paths(&id, &["side.txt".to_string()])
            .expect("stage side");
        let side_commit = engine
            .commit(&id, "side commit", &CommitOpts::default())
            .expect("commit side");

        engine.checkout(&id, "main", false).expect("checkout main");
        let outcome = engine
            .cherry_pick(&id, &side_commit)
            .expect("cherry-pick side commit");

        let ApplyOutcome::Applied(new_head) = outcome else {
            panic!("expected applied cherry-pick, got {outcome:?}");
        };
        assert_eq!(head_oid(p).expect("head"), new_head);
        assert_eq!(
            std::fs::read_to_string(p.join("side.txt")).expect("read side file"),
            "side\n"
        );
    }

    #[test]
    fn revert_undoes_a_commit() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        std::fs::write(p.join("file1.txt"), "changed\n").expect("write");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage");
        let changed = engine
            .commit(&id, "change file1", &CommitOpts::default())
            .expect("commit change");

        let outcome = engine.revert(&id, &changed).expect("revert change");
        assert!(matches!(outcome, ApplyOutcome::Applied(_)));
        assert_eq!(
            std::fs::read_to_string(p.join("file1.txt")).expect("read file1"),
            "content 1\n",
            "revert restored the previous content"
        );
    }

    #[test]
    fn conflicting_cherry_pick_reports_paths_and_abort_restores_head() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        engine.create_branch(&id, "side", None).expect("branch");
        engine.checkout(&id, "side", false).expect("checkout side");
        std::fs::write(p.join("file1.txt"), "side edit\n").expect("write side");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage side");
        let side_commit = engine
            .commit(&id, "side edit", &CommitOpts::default())
            .expect("commit side");

        engine.checkout(&id, "main", false).expect("checkout main");
        std::fs::write(p.join("file1.txt"), "main edit\n").expect("write main");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage main");
        engine
            .commit(&id, "main edit", &CommitOpts::default())
            .expect("commit main");
        let before = head_oid(p).expect("head before");

        let outcome = engine
            .cherry_pick(&id, &side_commit)
            .expect("conflicting cherry-pick should report conflicts");
        assert_eq!(
            outcome,
            ApplyOutcome::Conflicts(vec!["file1.txt".to_string()])
        );

        engine.sequencer_abort(&id).expect("sequencer abort");
        assert_eq!(head_oid(p).expect("head after abort"), before);
        let wt = engine.status(&id).expect("status after abort");
        assert!(
            wt.staged.is_empty() && wt.unstaged.is_empty() && wt.untracked.is_empty(),
            "abort restores a clean tree: {wt:?}"
        );
    }

    #[test]
    fn rebase_replays_branch_commits_onto_target() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        engine.create_branch(&id, "topic", None).expect("branch");
        engine
            .checkout(&id, "topic", false)
            .expect("checkout topic");
        std::fs::write(p.join("topic.txt"), "topic\n").expect("write topic");
        engine
            .stage_paths(&id, &["topic.txt".to_string()])
            .expect("stage topic");
        engine
            .commit(&id, "topic commit", &CommitOpts::default())
            .expect("commit topic");

        engine.checkout(&id, "main", false).expect("checkout main");
        std::fs::write(p.join("main.txt"), "main\n").expect("write main");
        engine
            .stage_paths(&id, &["main.txt".to_string()])
            .expect("stage main");
        engine
            .commit(&id, "main commit", &CommitOpts::default())
            .expect("commit main");

        let outcome = engine.rebase(&id, "topic", "main").expect("rebase topic");
        assert_eq!(outcome, RebaseOutcome::Rebased);
        assert!(
            is_ancestor(p, "main", "topic").expect("ancestor check"),
            "topic should be replayed on top of main"
        );
        assert_eq!(
            std::fs::read_to_string(p.join("topic.txt")).expect("read topic"),
            "topic\n"
        );
    }

    #[test]
    fn conflicting_rebase_reports_paths_and_abort_restores_branch() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        engine.create_branch(&id, "topic", None).expect("branch");
        engine
            .checkout(&id, "topic", false)
            .expect("checkout topic");
        std::fs::write(p.join("file1.txt"), "topic edit\n").expect("write topic");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage topic");
        engine
            .commit(&id, "topic edit", &CommitOpts::default())
            .expect("commit topic");
        let topic_head = head_oid(p).expect("topic head");

        engine.checkout(&id, "main", false).expect("checkout main");
        std::fs::write(p.join("file1.txt"), "main edit\n").expect("write main");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage main");
        engine
            .commit(&id, "main edit", &CommitOpts::default())
            .expect("commit main");

        let outcome = engine
            .rebase(&id, "topic", "main")
            .expect("conflicting rebase should report conflicts");
        assert_eq!(
            outcome,
            RebaseOutcome::Conflicts(vec!["file1.txt".to_string()])
        );

        engine.rebase_abort(&id).expect("rebase abort");
        assert_eq!(head_oid(p).expect("head after abort"), topic_head);
        let wt = engine.status(&id).expect("status after abort");
        assert!(
            wt.staged.is_empty() && wt.unstaged.is_empty() && wt.untracked.is_empty(),
            "abort restores a clean tree: {wt:?}"
        );
    }

    /// Drive a real merge conflict, then resolve it with the PH3-001 engine
    /// ops: parse regions, read the three index-stage sides, take-ours, mark
    /// resolved, and assert the file is unconflicted, staged, and our content.
    #[test]
    fn conflict_resolve_take_ours_marks_file_resolved_and_staged() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open the fixture repo");

        // side branch edits file1.txt one way ...
        engine.create_branch(&id, "side", None).expect("branch");
        engine.checkout(&id, "side", false).expect("checkout side");
        std::fs::write(p.join("file1.txt"), "their side\n").expect("write side");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage side");
        engine
            .commit(&id, "side edit", &CommitOpts::default())
            .expect("commit side");

        // ... and main edits it another, producing a conflict on merge.
        engine.checkout(&id, "main", false).expect("checkout main");
        std::fs::write(p.join("file1.txt"), "our side\n").expect("write main");
        engine
            .stage_paths(&id, &["file1.txt".to_string()])
            .expect("stage main");
        engine
            .commit(&id, "main edit", &CommitOpts::default())
            .expect("commit main");

        let outcome = engine
            .merge(&id, "side", &MergeOpts::default())
            .expect("conflicting merge");
        assert_eq!(
            outcome,
            MergeOutcome::Conflicts(vec!["file1.txt".to_string()])
        );

        // conflict_state reports a merge; list_conflicts surfaces the path.
        assert_eq!(
            engine.conflict_state(&id).expect("state"),
            ConflictState::Merge
        );
        assert_eq!(
            engine.list_conflicts(&id).expect("list"),
            vec!["file1.txt".to_string()]
        );

        // The working file carries markers and parses into a region.
        let parsed = engine.parse_conflict(&id, "file1.txt").expect("parse");
        assert!(
            parsed
                .segments
                .iter()
                .any(|s| matches!(s, lady_proto::ConflictSegment::Conflict(_))),
            "expected a conflict region in {parsed:?}"
        );

        // Index stages give us ours/theirs (no base for a modify/modify off a
        // shared ancestor where the ancestor content differs from both).
        let sides = engine.conflict_sides(&id, "file1.txt").expect("sides");
        assert_eq!(sides.ours.as_deref(), Some("our side\n"));
        assert_eq!(sides.theirs.as_deref(), Some("their side\n"));

        // Take ours, mark resolved.
        engine.take_ours(&id, "file1.txt").expect("take ours");
        assert_eq!(
            std::fs::read_to_string(p.join("file1.txt")).expect("read resolved"),
            "our side\n"
        );
        engine
            .mark_resolved(&id, "file1.txt")
            .expect("mark resolved");

        // No conflicts remain; the file is staged (index stage 0, no unmerged
        // entries). The resolved content equals HEAD here, so it shows no diff
        // in status — the authoritative "resolved & staged" signal is that the
        // unmerged index for the path is cleared.
        assert!(
            engine.list_conflicts(&id).expect("list after").is_empty(),
            "no conflicts should remain"
        );
        let unmerged = Command::new("git")
            .current_dir(p)
            .args(["ls-files", "-u", "--", "file1.txt"])
            .output()
            .expect("git ls-files -u");
        assert!(
            unmerged.stdout.is_empty(),
            "file should have no unmerged index entries after mark_resolved"
        );
        let wt = engine.status(&id).expect("status after resolve");
        assert!(
            !wt.unstaged
                .iter()
                .chain(wt.staged.iter())
                .any(|f| f.kind == ChangeKind::Conflicted),
            "no conflict entries should remain: {wt:?}"
        );

        // conflict_abort routes to merge --abort cleanly even mid-merge.
        engine.conflict_abort(&id).expect("conflict abort");
        assert_eq!(
            engine.conflict_state(&id).expect("state after abort"),
            ConflictState::None
        );
    }

    // ── Interactive rebase (PH3-003) ──────────────────────────────────────────

    /// Capture stdout of a git command in `dir` (test helper).
    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(out.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Add a commit writing `name` with `content`; return its Oid.
    fn add_commit(engine: &GixEngine, id: &RepoId, p: &Path, name: &str, content: &str) -> Oid {
        std::fs::write(p.join(name), content).expect("write file");
        engine
            .stage_paths(id, &[name.to_string()])
            .expect("stage file");
        engine
            .commit(id, &format!("add {name}"), &CommitOpts::default())
            .expect("commit file")
    }

    #[test]
    fn interactive_rebase_squashes_two_commits() {
        use lady_proto::RebaseAction;
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let base = head_oid(p).expect("base");
        let a = add_commit(&engine, &id, p, "a.txt", "A\n");
        let b = add_commit(&engine, &id, p, "b.txt", "B\n");

        let plan = vec![
            RebaseStep {
                oid: a,
                action: RebaseAction::Pick,
                message: None,
            },
            RebaseStep {
                oid: b,
                action: RebaseAction::Squash,
                message: None,
            },
        ];
        let outcome = engine
            .rebase_interactive(&id, base.as_str(), &plan)
            .expect("interactive squash");
        assert_eq!(outcome, RebaseOutcome::Rebased);

        // One commit since base, holding both files.
        let count = git_out(
            p,
            &["rev-list", "--count", &format!("{}..HEAD", base.as_str())],
        );
        assert_eq!(count, "1", "two commits squashed into one");
        assert!(p.join("a.txt").exists() && p.join("b.txt").exists());
        assert_eq!(
            engine.conflict_state(&id).expect("state"),
            ConflictState::None
        );
    }

    #[test]
    fn interactive_rebase_drops_a_commit() {
        use lady_proto::RebaseAction;
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let base = head_oid(p).expect("base");
        let a = add_commit(&engine, &id, p, "a.txt", "A\n");
        let b = add_commit(&engine, &id, p, "b.txt", "B\n");

        let plan = vec![
            RebaseStep {
                oid: a,
                action: RebaseAction::Pick,
                message: None,
            },
            RebaseStep {
                oid: b,
                action: RebaseAction::Drop,
                message: None,
            },
        ];
        let outcome = engine
            .rebase_interactive(&id, base.as_str(), &plan)
            .expect("interactive drop");
        assert_eq!(outcome, RebaseOutcome::Rebased);

        let count = git_out(
            p,
            &["rev-list", "--count", &format!("{}..HEAD", base.as_str())],
        );
        assert_eq!(count, "1", "dropped commit removed");
        assert!(p.join("a.txt").exists(), "kept commit's file present");
        assert!(!p.join("b.txt").exists(), "dropped commit's file gone");
    }

    #[test]
    fn interactive_rebase_reorders_two_commits() {
        use lady_proto::RebaseAction;
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let base = head_oid(p).expect("base");
        let a = add_commit(&engine, &id, p, "a.txt", "A\n");
        let b = add_commit(&engine, &id, p, "b.txt", "B\n");

        // Original tip is "add b.txt"; reorder so "add a.txt" ends up on top.
        let before_tip = git_out(p, &["log", "-1", "--format=%s"]);
        assert_eq!(before_tip, "add b.txt");

        let plan = vec![
            RebaseStep {
                oid: b,
                action: RebaseAction::Pick,
                message: None,
            },
            RebaseStep {
                oid: a,
                action: RebaseAction::Pick,
                message: None,
            },
        ];
        let outcome = engine
            .rebase_interactive(&id, base.as_str(), &plan)
            .expect("interactive reorder");
        assert_eq!(outcome, RebaseOutcome::Rebased);

        let after_tip = git_out(p, &["log", "-1", "--format=%s"]);
        assert_eq!(after_tip, "add a.txt", "reorder put a.txt on top");
        assert!(p.join("a.txt").exists() && p.join("b.txt").exists());
    }

    #[test]
    fn rebase_range_returns_onto_and_ordered_commits() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let base = head_oid(p).expect("base"); // tip of fixture (commit 3)
        let a = add_commit(&engine, &id, p, "a.txt", "A\n");
        let _b = add_commit(&engine, &id, p, "b.txt", "B\n");

        // "Rebase interactive from `a`": onto = a^ (== base), commits = [a, b].
        let (onto, commits) = engine.rebase_range(&id, &a).expect("range");
        assert_eq!(onto, base, "onto is the parent of the start commit");
        assert_eq!(commits.len(), 2, "two commits in the range");
        assert_eq!(commits[0].summary, "add a.txt", "oldest first");
        assert_eq!(commits[1].summary, "add b.txt");
    }

    #[test]
    fn lfs_tracked_file_round_trips_pointer_in_index_real_bytes_in_worktree() {
        if !lfs_available() {
            eprintln!("git-lfs unavailable — skipping LFS round-trip test");
            return;
        }
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // Enable LFS locally + track *.bin, then commit a binary file.
        git(p, &["lfs", "install", "--local"]);
        engine.lfs_track(&id, "*.bin").expect("lfs track");
        git(p, &["add", ".gitattributes"]);
        git(p, &["commit", "-q", "-m", "track bin with lfs"]);

        let real_bytes = b"\x00\x01\x02LADY-LFS-PAYLOAD\xff";
        std::fs::write(p.join("asset.bin"), real_bytes).expect("write bin");
        git(p, &["add", "asset.bin"]);
        git(p, &["commit", "-q", "-m", "add asset"]);

        // The committed blob (HEAD) is an LFS pointer, not the real bytes.
        let head_blob = git_out(p, &["show", "HEAD:asset.bin"]);
        assert!(
            head_blob.contains("version https://git-lfs"),
            "committed blob should be an LFS pointer, got: {head_blob}"
        );
        // The working-tree file holds the real (materialized) bytes.
        assert_eq!(
            std::fs::read(p.join("asset.bin")).expect("read worktree bin"),
            real_bytes,
            "working tree should hold the real bytes (smudge filter ran)"
        );

        // Engine status reflects availability, the pattern, and the file.
        let status = engine.lfs_status(&id).expect("lfs status");
        assert!(status.available);
        assert!(
            status.patterns.iter().any(|p| p == "*.bin"),
            "tracked patterns: {:?}",
            status.patterns
        );
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "asset.bin" && f.downloaded),
            "lfs files: {:?}",
            status.files
        );
    }

    #[test]
    fn submodule_add_update_and_status() {
        // A standalone repo to use as the submodule source.
        let sub_src = tempfile::tempdir().expect("sub src");
        let sp = sub_src.path();
        git(sp, &["init", "-q", "-b", "main"]);
        git(sp, &["config", "user.name", "Sub"]);
        git(sp, &["config", "user.email", "s@s.com"]);
        git(sp, &["config", "commit.gpgsign", "false"]);
        std::fs::write(sp.join("lib.txt"), "lib\n").expect("write");
        git(sp, &["add", "."]);
        git(sp, &["commit", "-q", "-m", "sub init"]);

        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // Local submodule transport (the `file` protocol) is allowed by
        // add_submodule itself via `-c protocol.file.allow=always`.
        let url = sp.to_str().expect("sub src path");
        engine
            .add_submodule(&id, url, "libs/sub")
            .expect("add submodule");
        git(p, &["commit", "-q", "-m", "add submodule"]);

        let subs = engine.list_submodules(&id).expect("list submodules");
        let sub = subs
            .iter()
            .find(|s| s.path == "libs/sub")
            .expect("submodule listed");
        assert!(sub.initialized, "added submodule is initialized: {sub:?}");
        assert_eq!(sub.url, url, "url read from .gitmodules");
        assert!(!sub.sha.is_empty(), "pinned sha recorded");
        assert!(
            p.join("libs/sub/lib.txt").exists(),
            "submodule content checked out"
        );

        // Update is a no-op here but must succeed.
        engine.update_submodules(&id).expect("update submodules");
    }

    #[test]
    fn git_flow_feature_start_and_finish_merges_into_develop() {
        use lady_proto::FlowKind;
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // Init git-flow (master = main, develop created from it).
        let cfg = FlowConfig {
            master: "main".to_string(),
            ..FlowConfig::default()
        };
        engine.flow_init(&id, &cfg).expect("flow init");
        let loaded = engine.flow_config(&id).expect("flow config");
        assert!(loaded.initialized);
        assert_eq!(loaded.develop, "develop");

        // Start a feature, add a commit on it.
        let branch = engine
            .flow_start(&id, FlowKind::Feature, "login")
            .expect("flow start");
        assert_eq!(branch, "feature/login");
        std::fs::write(p.join("login.rs"), "fn login() {}\n").expect("write");
        git(p, &["add", "login.rs"]);
        git(p, &["commit", "-q", "-m", "add login"]);

        // Finish: merges into develop and deletes the feature branch.
        engine
            .flow_finish(&id, FlowKind::Feature, "login")
            .expect("flow finish");

        // HEAD is develop, the feature file is present, the branch is gone.
        let head_branch = git_out(p, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(head_branch, "develop");
        assert!(p.join("login.rs").exists(), "feature merged into develop");
        let branches = git_out(p, &["branch", "--list", "feature/login"]);
        assert!(branches.is_empty(), "feature branch deleted: {branches:?}");
        // The merge was --no-ff (a merge commit exists on develop).
        let subject = git_out(p, &["log", "-1", "--format=%s"]);
        assert!(
            subject.contains("Merge branch 'feature/login'"),
            "subject: {subject}"
        );
    }

    #[test]
    fn add_remote_then_list_remote_urls() {
        let dir = fixture_repo();
        let engine = GixEngine::new();
        let id = engine.open(dir.path()).expect("open");
        assert!(engine.list_remote_urls(&id).expect("list").is_empty());
        engine
            .add_remote(&id, "origin", "https://github.com/o/r.git")
            .expect("add remote");
        let urls = engine.list_remote_urls(&id).expect("list after");
        assert_eq!(urls, vec!["https://github.com/o/r.git".to_string()]);
    }

    #[test]
    fn scripted_bisect_converges_to_the_known_bad_commit() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // good_tip is the current (clean) tip. Build a chain; at commit 4 we
        // introduce bug.txt, which then persists — so commit 4 is "first bad".
        let good = head_oid(p).expect("good tip");
        let mut first_bad: Option<Oid> = None;
        for i in 1..=6 {
            if i == 4 {
                std::fs::write(p.join("bug.txt"), "bug\n").expect("write bug");
            }
            std::fs::write(p.join(format!("c{i}.txt")), format!("{i}\n")).expect("write");
            git(p, &["add", "-A"]);
            git(p, &["commit", "-q", "-m", &format!("c{i}")]);
            if i == 4 {
                first_bad = Some(head_oid(p).expect("bad commit"));
            }
        }
        let bad = head_oid(p).expect("bad tip");
        let first_bad = first_bad.expect("captured first-bad");

        // Drive bisect: mark bad when bug.txt is present at the tested commit.
        let mut state = engine.bisect_start(&id, &bad, &good).expect("bisect start");
        let mut guard = 0;
        while state.suspected.is_none() {
            guard += 1;
            assert!(guard < 20, "bisect should converge quickly");
            let mark = if p.join("bug.txt").exists() {
                "bad"
            } else {
                "good"
            };
            state = engine.bisect_mark(&id, mark).expect("bisect mark");
        }

        assert_eq!(
            state.suspected.as_ref(),
            Some(&first_bad),
            "bisect should pinpoint the commit that introduced bug.txt"
        );

        engine.bisect_reset(&id).expect("bisect reset");
        assert!(
            !p.join(".git").join("BISECT_START").exists(),
            "reset exits bisect"
        );
    }

    #[test]
    fn reflog_surfaces_dangling_commit_and_allows_branch_recovery() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // Make a commit, then hard-reset past it so it becomes dangling.
        let lost = add_commit(&engine, &id, p, "lost.txt", "precious\n");
        git(p, &["reset", "--hard", "HEAD~1"]);
        assert_ne!(head_oid(p).expect("head"), lost, "commit was reset away");

        // The reflog still records the lost commit.
        let log = engine.reflog(&id, "HEAD").expect("reflog");
        assert!(
            log.iter().any(|e| e.oid == lost),
            "dangling commit should appear in the reflog: {log:?}"
        );
        // The reset entry should be classified by action.
        assert!(
            log.iter().any(|e| e.action == "reset"),
            "a reset action should be present: {log:?}"
        );

        // Recover it by creating a branch at its oid.
        engine
            .create_branch(&id, "recovered", Some(lost.as_str()))
            .expect("create recovery branch");
        let recovered_tip = git_out(p, &["rev-parse", "recovered"]);
        assert_eq!(
            recovered_tip,
            lost.as_str(),
            "branch points at the lost commit"
        );
    }

    #[test]
    fn worktree_add_list_remove_roundtrip() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // Worktree dir is a sibling temp path (must not already exist).
        let wt_parent = tempfile::tempdir().expect("wt parent");
        let wt_path = wt_parent.path().join("feature-wt");
        let wt_str = wt_path.to_str().expect("wt path utf8");

        // Initially only the main worktree is listed.
        let before = engine.list_worktrees(&id).expect("list before");
        assert_eq!(before.len(), 1, "only the main worktree exists: {before:?}");
        assert_eq!(before[0].display_name, "main");
        assert!(before[0].is_main);
        assert!(before[0].selected);

        // Add a worktree on a new branch and confirm it lists.
        engine
            .add_worktree(&id, wt_str, Some("feature"), true)
            .expect("add worktree");
        let listed = engine.list_worktrees(&id).expect("list after add");
        assert_eq!(listed.len(), 2, "main + new worktree: {listed:?}");
        let family = engine.repository_family(&id).expect("repository family");
        assert_eq!(family.worktrees.len(), 2);
        assert_eq!(family.main.display_name, "main");
        assert!(
            family.id.as_str().ends_with(".git"),
            "family id is the common git dir: {:?}",
            family.id
        );
        let added = listed
            .iter()
            .find(|w| w.path == wt_path.canonicalize().unwrap().to_string_lossy())
            .or_else(|| {
                listed
                    .iter()
                    .find(|w| w.branch.as_deref() == Some("feature"))
            })
            .expect("added worktree present");
        assert_eq!(added.display_name, "feature");
        assert_eq!(added.branch.as_deref(), Some("feature"));
        assert!(added.head.is_some(), "worktree HEAD resolved");
        assert!(!added.is_main);
        assert!(!added.missing);

        // Remove it; back to just the main worktree.
        engine
            .remove_worktree(&id, wt_str)
            .expect("remove worktree");
        let after = engine.list_worktrees(&id).expect("list after remove");
        assert_eq!(after.len(), 1, "worktree removed: {after:?}");
    }

    /// SSH commit signing end-to-end: generate an ephemeral ssh key + an
    /// allowed-signers file in a tempdir, configure ssh signing, sign a commit
    /// with `CommitOpts::sign`, and assert verification reads Good (PH3-005).
    /// Skips cleanly when `ssh-keygen` is unavailable.
    /// Parse `git --version` into (major, minor); `None` if unparseable.
    fn git_version() -> Option<(u32, u32)> {
        let out = Command::new("git").arg("--version").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let ver = text.split_whitespace().nth(2)?;
        let mut parts = ver.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        Some((major, minor))
    }

    #[test]
    fn ssh_signed_commit_verifies_good() {
        // SSH commit signing requires git >= 2.34; skip on older toolchains.
        match git_version() {
            Some((maj, min)) if (maj, min) >= (2, 34) => {}
            _ => {
                eprintln!("git < 2.34 — skipping ssh_signed_commit_verifies_good");
                return;
            }
        }

        let dir = fixture_repo();
        let p = dir.path();

        // Generate an ed25519 key; skip the test if ssh-keygen isn't present.
        let key = p.join("id_ed25519");
        let keygen = Command::new("ssh-keygen")
            .args([
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                "lady-test",
                "-q",
                "-f",
                key.to_str().expect("key path"),
            ])
            .status();
        match keygen {
            Ok(s) if s.success() => {}
            _ => {
                eprintln!("ssh-keygen unavailable — skipping ssh_signed_commit_verifies_good");
                return;
            }
        }

        let pub_key = std::fs::read_to_string(p.join("id_ed25519.pub")).expect("read pubkey");
        // allowed_signers: "<principal> <keytype> <key> [comment]" — the
        // principal must match the committer email used by the fixture.
        let signers = p.join("allowed_signers");
        std::fs::write(&signers, format!("test@example.com {pub_key}")).expect("write signers");

        // Configure ssh signing for this repo only.
        git(p, &["config", "gpg.format", "ssh"]);
        git(
            p,
            &[
                "config",
                "user.signingkey",
                key.with_extension("pub").to_str().expect("pub path"),
            ],
        );
        git(
            p,
            &[
                "config",
                "gpg.ssh.allowedSignersFile",
                signers.to_str().expect("signers path"),
            ],
        );

        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // Force-sign a fresh commit via the engine (-S path).
        std::fs::write(p.join("signed.txt"), "signed\n").expect("write");
        engine
            .stage_paths(&id, &["signed.txt".to_string()])
            .expect("stage");
        let oid = engine
            .commit(
                &id,
                "signed commit",
                &CommitOpts {
                    amend: false,
                    sign: true,
                },
            )
            .expect("signed commit");

        let statuses = engine
            .signature_statuses(&id, &[oid])
            .expect("signature statuses");
        assert_eq!(
            statuses,
            vec![SignatureStatus::Good],
            "ssh-signed commit should verify Good"
        );

        // The previous (unsigned) fixture commit reads None.
        let prev = git_out(p, &["rev-parse", "HEAD~1"]);
        let base = engine
            .signature_statuses(&id, &[Oid::from(prev)])
            .expect("status");
        assert_eq!(base, vec![SignatureStatus::None]);
    }

    #[test]
    fn conflicting_interactive_reorder_stops_then_aborts_cleanly() {
        use lady_proto::RebaseAction;
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let base = head_oid(p).expect("base");
        // Both commits edit the SAME file in sequence; reordering them makes the
        // second patch fail to apply → conflict.
        let a = add_commit(&engine, &id, p, "file1.txt", "A\n");
        let b = add_commit(&engine, &id, p, "file1.txt", "B\n");
        let before = head_oid(p).expect("tip before rebase");

        let plan = vec![
            RebaseStep {
                oid: b,
                action: RebaseAction::Pick,
                message: None,
            },
            RebaseStep {
                oid: a,
                action: RebaseAction::Pick,
                message: None,
            },
        ];
        let outcome = engine
            .rebase_interactive(&id, base.as_str(), &plan)
            .expect("interactive conflicting reorder");
        assert!(
            matches!(outcome, RebaseOutcome::Conflicts(_)),
            "expected conflicts, got {outcome:?}"
        );
        assert_eq!(
            engine.conflict_state(&id).expect("state mid-rebase"),
            ConflictState::Rebase
        );

        engine.rebase_abort(&id).expect("abort");
        assert_eq!(head_oid(p).expect("head after abort"), before);
        let wt = engine.status(&id).expect("status after abort");
        assert!(
            wt.staged.is_empty() && wt.unstaged.is_empty() && wt.untracked.is_empty(),
            "abort restores a clean tree: {wt:?}"
        );
        assert_eq!(
            engine.conflict_state(&id).expect("state after abort"),
            ConflictState::None
        );
    }

    // ── Plan 4: context-menu engine primitives ─────────────────────────────

    #[test]
    fn rename_branch_renames() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");
        engine.create_branch(&id, "old-name", None).expect("create");

        engine
            .rename_branch(&id, "old-name", "new-name")
            .expect("rename");
        assert_eq!(git_out(p, &["branch", "--list", "old-name"]), "");
        assert!(git_out(p, &["branch", "--list", "new-name"]).contains("new-name"));
    }

    #[test]
    fn branches_ahead_behind_maps_tracked_branches() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        git(root, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        let remote = root.join("remote.git");
        let a = root.join("a");
        git(
            root,
            &["clone", "-q", remote.to_str().unwrap(), a.to_str().unwrap()],
        );
        git(&a, &["config", "user.name", "Lady Test"]);
        git(&a, &["config", "user.email", "test@example.com"]);
        git(&a, &["config", "commit.gpgsign", "false"]);
        std::fs::write(a.join("f.txt"), "v1\n").expect("write");
        git(&a, &["add", "."]);
        git(&a, &["commit", "-q", "-m", "first"]);

        let engine = GixEngine::new();
        let id = engine.open(&a).expect("open");
        let mut sink = |_: &str| {};
        engine
            .push(
                &id,
                Some("origin"),
                Some("main"),
                true,
                false,
                &GitAuth::none(),
                &mut sink,
            )
            .expect("push");

        // A local branch with no upstream is omitted from the map.
        engine.create_branch(&id, "feature", None).expect("branch");

        // One unpushed commit on main → ahead 1.
        std::fs::write(a.join("f.txt"), "v2\n").expect("write");
        git(&a, &["add", "."]);
        git(&a, &["commit", "-q", "-m", "second"]);

        let map = engine.branches_ahead_behind(&id).expect("ahead/behind");
        assert_eq!(
            map.get("main"),
            Some(&AheadBehind {
                ahead: 1,
                behind: 0
            })
        );
        assert_eq!(
            map.get("origin/main"),
            Some(&AheadBehind {
                ahead: 1,
                behind: 0
            })
        );
        assert!(!map.contains_key("feature"), "untracked branch omitted");
    }

    #[test]
    fn branch_upstream_round_trips() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");
        // A second branch acts as the "upstream" target without needing a remote.
        engine.create_branch(&id, "track", None).expect("create");

        assert_eq!(
            engine.branch_upstream(&id, "track").expect("read unset"),
            None,
            "no upstream by default"
        );

        engine
            .set_branch_upstream(&id, "track", Some("main"))
            .expect("set upstream");
        assert_eq!(
            engine.branch_upstream(&id, "track").expect("read set"),
            Some("main".to_string())
        );

        engine
            .set_branch_upstream(&id, "track", None)
            .expect("unset upstream");
        assert_eq!(
            engine
                .branch_upstream(&id, "track")
                .expect("read after unset"),
            None
        );
    }

    #[test]
    fn fast_forward_branch_advances_only_when_ancestor() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        // `behind` points at HEAD~2; main is two commits ahead (fast-forwardable).
        let target = git_out(p, &["rev-parse", "main"]);
        git(p, &["branch", "behind", "main~2"]);

        engine
            .fast_forward_branch(&id, "behind", "main")
            .expect("ff succeeds for an ancestor");
        assert_eq!(git_out(p, &["rev-parse", "behind"]), target);

        // Diverge: a branch off main~1 with its own commit is NOT an ancestor.
        git(p, &["checkout", "-q", "-b", "diverged", "main~1"]);
        std::fs::write(p.join("d.txt"), "d\n").expect("write");
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "diverged commit"]);
        git(p, &["checkout", "-q", "main"]);

        let err = engine
            .fast_forward_branch(&id, "diverged", "main")
            .expect_err("non-ancestor must error");
        assert!(
            err.to_string().contains("not fast-forwardable"),
            "clear error: {err}"
        );
    }

    #[test]
    fn discard_files_restores_tracked_changes() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        let original = std::fs::read_to_string(p.join("file1.txt")).expect("read original");
        std::fs::write(p.join("file1.txt"), "scribbled over\n").expect("write");
        assert!(!engine.status(&id).unwrap().unstaged.is_empty());

        engine
            .discard_files(&id, &["file1.txt".to_string()])
            .expect("discard");
        assert_eq!(
            std::fs::read_to_string(p.join("file1.txt")).unwrap(),
            original
        );
        assert!(engine.status(&id).unwrap().unstaged.is_empty());
    }

    #[test]
    fn stash_paths_stashes_only_named_files() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        std::fs::write(p.join("file1.txt"), "edit one\n").expect("write");
        std::fs::write(p.join("file2.txt"), "edit two\n").expect("write");

        engine
            .stash_paths(&id, Some("just file1"), false, &["file1.txt".to_string()])
            .expect("stash one");

        // file1 reverted (stashed away); file2 still dirty.
        assert_eq!(
            git_out(p, &["show", "HEAD:file1.txt"]),
            std::fs::read_to_string(p.join("file1.txt")).unwrap().trim()
        );
        assert_eq!(
            std::fs::read_to_string(p.join("file2.txt")).unwrap(),
            "edit two\n"
        );
        assert!(git_out(p, &["stash", "list"]).contains("just file1"));
    }

    #[test]
    fn export_patch_writes_uncommitted_diff() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        std::fs::write(p.join("file1.txt"), "patched line\n").expect("write");
        let dest = p.join("out.patch");
        engine
            .export_patch(&id, &["file1.txt".to_string()], dest.to_str().unwrap())
            .expect("export");

        let patch = std::fs::read_to_string(&dest).expect("read patch");
        assert!(patch.contains("--- a/file1.txt"), "patch header: {patch}");
        assert!(patch.contains("+patched line"), "patch body: {patch}");
    }

    #[test]
    fn add_to_gitignore_appends_without_dupes() {
        let dir = fixture_repo();
        let p = dir.path();
        let engine = GixEngine::new();
        let id = engine.open(p).expect("open");

        engine
            .add_to_gitignore(&id, &["*.tmp".to_string(), "build/".to_string()])
            .expect("first add");
        // Re-add one existing + one new: existing must not duplicate.
        engine
            .add_to_gitignore(&id, &["*.tmp".to_string(), "*.log".to_string()])
            .expect("second add");

        let body = std::fs::read_to_string(p.join(".gitignore")).expect("read");
        assert_eq!(body.matches("*.tmp").count(), 1, "no dup: {body}");
        assert!(body.contains("build/"));
        assert!(body.contains("*.log"));
    }
}
