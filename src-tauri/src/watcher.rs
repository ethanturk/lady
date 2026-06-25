//! Filesystem watcher backing the live repo refresh.
//!
//! Replaces the old 2-second poll: instead of re-running `status`/`list_refs`
//! on a timer, the backend watches the active repo's working tree (and, for a
//! linked worktree, the shared common git dir) and emits a single
//! `repo-fs-changed` event when something relevant changes. The frontend listens
//! and refreshes on demand. Events are debounced so an editor save or a `git`
//! command — which touches many files at once — yields one refresh, not a burst.
//!
//! We use the "mini" debouncer deliberately: it registers the OS watch and
//! coalesces events without the full debouncer's recursive file-id pre-scan,
//! which would `stat` every file under the worktree (seconds on a repo with a
//! large `node_modules`). We only need a coarse "something changed" signal, so
//! the precise rename tracking the full debouncer buys is not worth its cost.
//!
//! Desktop only. On mobile the `watch` entry point returns an error and the
//! frontend falls back to interval polling.

use std::collections::HashMap;
use std::sync::Mutex;

use lady_proto::RepoId;

/// A live watcher handle. Dropping it stops the underlying watcher thread, so
/// re-watching or unwatching a repo is just a map insert/remove.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
type WatchHandle =
    notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>;
#[cfg(any(target_os = "android", target_os = "ios"))]
type WatchHandle = ();

/// Active filesystem watchers, keyed by the repo they observe. Tauri-managed
/// state; one entry per currently-watched repo (in practice the active one).
#[derive(Default)]
pub struct RepoWatchers {
    inner: Mutex<HashMap<RepoId, WatchHandle>>,
}

impl RepoWatchers {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Stop watching `repo` (no-op if it was not being watched). Dropping the stored
/// handle tears down the watcher thread.
pub fn unwatch(watchers: &RepoWatchers, repo: &RepoId) {
    watchers
        .inner
        .lock()
        .expect("RepoWatchers mutex poisoned")
        .remove(repo);
}

/// Paths whose changes never affect `git status`/refs, so refreshing on them is
/// wasted work: git's internal object/lfs churn, lock/temp files, and the large
/// ignored build/dependency trees. Filtering here keeps an editor's node_modules
/// rebuild or a `git gc` from spamming refreshes.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn is_relevant(p: &std::path::Path) -> bool {
    let s = p.to_string_lossy();
    const IGNORE_FRAGMENTS: [&str; 4] = ["/.git/objects", "/.git/lfs", "/node_modules/", "/target/"];
    if IGNORE_FRAGMENTS.iter().any(|frag| s.contains(frag)) {
        return false;
    }
    !(s.ends_with(".lock") || s.ends_with(".tmp") || s.ends_with('~'))
}

/// Start watching `repo`'s `workdir` (recursively) plus its `common_dir` when
/// that lives outside the worktree (linked worktrees keep refs in the shared
/// dir). Replaces any existing watcher for the repo. Emits `repo-fs-changed`
/// with the repo handle when a relevant change is debounced.
///
/// Registering the OS watch can block (inotify walks the tree to add per-dir
/// watches on Linux), so callers run this off the main thread (the command is
/// async).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn watch(
    watchers: &RepoWatchers,
    repo: RepoId,
    workdir: std::path::PathBuf,
    common_dir: std::path::PathBuf,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use std::time::Duration;

    use notify_debouncer_mini::notify::RecursiveMode;
    use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
    use tauri::Emitter;

    let emit_repo = repo.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(400),
        move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            if events.iter().any(|ev| is_relevant(&ev.path)) {
                // Payload is the repo handle string; the frontend ignores events
                // for a repo that is no longer active.
                let _ = app.emit("repo-fs-changed", emit_repo.0.clone());
            }
        },
    )
    .map_err(|e| e.to_string())?;

    debouncer
        .watcher()
        .watch(&workdir, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    // Linked worktrees keep HEAD/refs/index in the shared common dir, which sits
    // outside the worktree. Watch it too (best-effort — a missing dir is fine).
    if !common_dir.starts_with(&workdir) {
        let _ = debouncer
            .watcher()
            .watch(&common_dir, RecursiveMode::Recursive);
    }

    watchers
        .inner
        .lock()
        .expect("RepoWatchers mutex poisoned")
        .insert(repo, debouncer);
    Ok(())
}

/// Mobile stub: filesystem watching is unavailable, so the caller falls back to
/// interval polling.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn watch(
    _watchers: &RepoWatchers,
    _repo: RepoId,
    _workdir: std::path::PathBuf,
    _common_dir: std::path::PathBuf,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    Err("filesystem watching is unavailable on this platform".to_string())
}
