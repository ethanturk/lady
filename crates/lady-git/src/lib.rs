//! `lady-git` — the engine abstraction for Lady's git reads.
//!
//! [`GitEngine`] is the single trait all read backends implement (gix today,
//! git2/shell later) per ADR-0003. This crate defines the contract only; the
//! gix-backed implementation arrives in US-006.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use lady_proto::{
    Blame, BlameLine, ChangeKind, CommitMeta, FileDiff, FileDiffKind, FileStatus, Oid, RefInfo,
    RefKind, RepoId, Signature, WorkingTree,
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
}

/// Run system `git` in `workdir` with `args`, returning its captured output.
///
/// On a non-zero exit, returns [`Error::Git`] carrying git's stderr verbatim
/// (ADR-0003 shell-out mutation tier; surface git's own messages faithfully).
pub(crate) fn run_git(workdir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let out = Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()
        .map_err(|e| Error::Git(format!("failed to run git: {e}")))?;
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

/// Run system `git` in `workdir`, feeding `input` to its stdin (used for
/// `git apply`, which reads the patch from stdin). Errors carry git's stderr.
pub(crate) fn run_git_stdin(workdir: &Path, args: &[&str], input: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .current_dir(workdir)
        .args(args)
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
}
