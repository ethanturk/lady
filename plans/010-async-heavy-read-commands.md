# Plan 010: Move heavy local read commands off the Tauri main thread

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- src-tauri/src/lib.rs`
> If `src-tauri/src/lib.rs` changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none (independent of 009; shares the same conversion pattern)
- **Category**: perf
- **Planned at**: commit `9666d42`, 2026-06-25

## Why this matters

In Tauri a synchronous `#[tauri::command] fn` runs **on the main (UI) thread**, so
any blocking work inside it freezes the WebView until it returns (Tauri docs:
*"Commands without the async keyword are executed on the main thread"*).

Several read-only git commands do work that **scales with repository size** and
runs synchronously on the main thread today:

- `walk_log` / `walk_log_graph` page the commit graph and compute lane layout
  across history — `walk_log_graph` also calls `list_refs` and builds maps.
- `blame` runs a full file blame; `file_history` walks every commit touching a
  path; `status` diffs the whole working tree; `diff` materializes a commit's
  file diffs.
- `signature_statuses` verifies signatures — on a signed history this spawns
  `gpg`/`ssh-keygen` **per commit**, which is slow enough to visibly stall a
  scroll through the graph.
- `reflog` reads the reflog for a ref.

On a large repository each of these can block the UI for hundreds of
milliseconds to several seconds. The codebase already made `watch_repo` async
for exactly this reason ("would freeze the UI for seconds on a large repo",
`src-tauri/src/lib.rs:898-900`). This plan applies that same pattern to the heavy
read commands.

## Current state

- `src-tauri/src/lib.rs` — all Tauri command handlers; thin wrappers over
  `engine: State<GixEngine>` (the managed, **blocking** `lady_git` engine).

- **Already-async exemplar to match** — `async fn` + `State<'_, …>` on every
  `State` param (`src-tauri/src/lib.rs:901-907`):
  ```rust
  #[tauri::command]
  async fn watch_repo(
      repo: RepoId,
      app: tauri::AppHandle,
      engine: State<'_, GixEngine>,
      watchers: State<'_, watcher::RepoWatchers>,
  ) -> Result<(), String> {
  ```

- The commands to convert, **as they exist today** (all synchronous `fn`):

  - `walk_log` (`:60-71`):
    ```rust
    #[tauri::command]
    fn walk_log(
        repo: RepoId,
        query: WalkLogQuery,
        engine: State<GixEngine>,
    ) -> Result<Vec<CommitMeta>, String> {
    ```
  - `walk_log_graph` (`:102-...`) — long body, leave it unchanged:
    ```rust
    #[tauri::command]
    fn walk_log_graph(
        repo: RepoId,
        query: WalkLogQuery,
        layout_state: Option<Vec<Option<String>>>,
        engine: State<GixEngine>,
    ) -> Result<WalkLogGraphResult, String> {
    ```
  - `diff` (`:187-189`):
    ```rust
    #[tauri::command]
    fn diff(repo: RepoId, commit: String, engine: State<GixEngine>) -> Result<Vec<FileDiff>, String> {
    ```
  - `diff_spec` (`:201-...`) — multi-arg variant directly below `diff`:
    ```rust
    #[tauri::command]
    fn diff_spec(
    ```
  - `blame` (`:216-226`):
    ```rust
    #[tauri::command]
    fn blame(
        repo: RepoId,
        path: String,
        at: Option<String>,
        engine: State<GixEngine>,
    ) -> Result<Blame, String> {
    ```
  - `file_history` (`:229-235`):
    ```rust
    #[tauri::command]
    fn file_history(
        repo: RepoId,
        path: String,
        engine: State<GixEngine>,
    ) -> Result<Vec<CommitMeta>, String> {
    ```
  - `status` (`:251-...`):
    ```rust
    #[tauri::command]
    fn status(repo: RepoId, engine: State<GixEngine>) -> Result<WorkingTree, String> {
    ```
  - `signature_statuses` (`:407-417`):
    ```rust
    #[tauri::command]
    fn signature_statuses(
        repo: RepoId,
        oids: Vec<String>,
        engine: State<GixEngine>,
    ) -> Result<Vec<lady_proto::SignatureStatus>, String> {
    ```
  - `reflog` (`:594-...`):
    ```rust
    #[tauri::command]
    fn reflog(
        repo: RepoId,
        refname: Option<String>,
        engine: State<GixEngine>,
    ) -> Result<Vec<lady_proto::ReflogEntry>, String> {
    ```

- **Why this is safe** (same as plan 009): `GixEngine` is `.manage()`d so it is
  already `Send + Sync + 'static`; `watch_repo` already borrows it as
  `State<'_, GixEngine>` in an async command; none of the bodies below contain an
  `.await`, so they stay identical; `tauri::generate_handler!` treats async and
  sync commands the same, so **the handler list needs no change**.

## Commands you will need

| Purpose   | Command                                                        | Expected on success         |
|-----------|---------------------------------------------------------------|-----------------------------|
| Compile   | `cargo build -p lady-app`                                      | exit 0, no errors           |
| Lint      | `cargo clippy -p lady-app --all-targets -- -D warnings`        | exit 0, no warnings         |
| Format    | `cargo fmt --all -- --check`                                   | exit 0                      |
| Tests     | `cargo test -p lady-app`                                       | all pass                    |

(From `AGENTS.md`. Backend-only change; the UI build need not run.)

## Scope

**In scope** (the only file you should modify):
- `src-tauri/src/lib.rs`

**Out of scope** (do NOT touch):
- Any crate under `crates/` — engine methods stay blocking.
- Any UI file under `ui/` — callers already `await invoke(...)`.
- The `tauri::generate_handler![...]` list — unchanged for async commands.
- Trivial read commands not listed here (`repo_dirty`, `list_files`,
  `list_refs`, `recent_messages`, `app_info`) — fast enough on the main thread;
  leave them synchronous to avoid churn.
- The network commands (handled by plan 009) and mutating commands (plan 011).

## Git workflow

- Branch: `advisor/010-async-heavy-read-commands`
- One commit is fine. Conventional Commits message, e.g.
  `perf(commands): run heavy read commands off the main thread`.
- Do NOT push or open a PR unless instructed.

## Steps

For **each** command below, apply the identical transformation: insert `async`
before `fn`, change every `State<T>` param to `State<'_, T>`, and leave the body
unchanged.

### Step 1: Convert the graph/log/diff read commands

Convert in `src-tauri/src/lib.rs`: `walk_log` (`:61`), `walk_log_graph` (`:103`),
`diff` (`:188`), `diff_spec` (`:202`).

**Verify**: `cargo build -p lady-app` → exit 0. (A `State` lifetime error means a
`State` param on that fn is still missing its `'_` — fix and rebuild.)

### Step 2: Convert the blame/history/status/signature/reflog commands

Convert: `blame` (`:217`), `file_history` (`:230`), `status` (`:252`),
`signature_statuses` (`:408`), `reflog` (`:594` — confirm the line with the
drift-check grep below; it sits just before `bisect_start`).

**Verify**: `cargo build -p lady-app` → exit 0.

### Step 3: Lint, format, test

**Verify**:
- `cargo clippy -p lady-app --all-targets -- -D warnings` → exit 0.
- `cargo fmt --all -- --check` → exit 0 (run `cargo fmt --all` first if needed).
- `cargo test -p lady-app` → all pass.

### Step 4: Confirm completeness

**Verify**:
```
grep -nE 'async fn (walk_log|walk_log_graph|diff|diff_spec|blame|file_history|status|signature_statuses|reflog)\b' src-tauri/src/lib.rs
```
Expected: 9 matching lines (one each — note `walk_log` and `walk_log_graph` are
distinct, and `diff` vs `diff_spec` are distinct).

## Test plan

No new automated tests — these are unchanged pass-throughs to `lady_git`; only
their execution thread changes, which the type system cannot assert and existing
tests don't cover. Verification is compilation + clippy + the existing
`cargo test -p lady-app` suite + the Step 4 grep.

Optional manual check for the reviewer: open a large signed repository, scroll
the commit graph, and confirm the UI no longer hitches while
`signature_statuses` / `walk_log_graph` pages load.

## Done criteria

ALL must hold:

- [ ] `cargo build -p lady-app` exits 0
- [ ] `cargo clippy -p lady-app --all-targets -- -D warnings` exits 0
- [ ] `cargo fmt --all -- --check` exits 0
- [ ] `cargo test -p lady-app` passes
- [ ] The Step 4 grep returns 9 `async fn` lines
- [ ] Only `src-tauri/src/lib.rs` is modified (`git status`)
- [ ] `plans/README.md` status row for 010 updated

## STOP conditions

Stop and report back (do not improvise) if:

- The drift check shows `src-tauri/src/lib.rs` changed since `9666d42` and a
  target command no longer matches its excerpt.
- The build fails with an error not resolved by adding a missing `'_` to a
  `State` param (e.g. a body holds a non-`Send` value, or an engine method is
  `!Send`). That means this command needs `spawn_blocking` instead — report it.
- The change appears to require editing any file outside `src-tauri/src/lib.rs`.

## Maintenance notes

- Same trade-off as plan 009: `async fn` with a blocking body uses an async
  worker thread, not the main thread. If concurrent heavy reads ever starve the
  worker pool, wrap the engine call in `tauri::async_runtime::spawn_blocking`.
- A reviewer should verify every converted command kept all its `State` params as
  `State<'_, T>` and that no `.await` was introduced.
- `signature_statuses` is the heaviest of this group (per-commit subprocess); if
  it is later cached or batched, that work composes with this change rather than
  conflicting.
