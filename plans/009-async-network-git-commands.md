# Plan 009: Move network git commands off the Tauri main thread

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

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: perf
- **Planned at**: commit `9666d42`, 2026-06-25

## Why this matters

In Tauri, a synchronous `#[tauri::command] fn` runs **on the main (UI) thread**.
Any blocking work inside it freezes the entire WebView — the window stops
repainting and stops responding to input until the call returns. The Tauri docs
are explicit: *"Commands without the async keyword are executed on the main
thread"* and *"Asynchronous commands are preferred … to perform heavy work in a
manner that doesn't result in UI freezes."*

Lady's git engine shells out to system `git` with **blocking** process calls
(`Command::output()`, `child.wait()` — see `crates/lady-git/src/lib.rs:780`,
`:941`). The network commands (`clone_repo`, `fetch`, `pull`, `push`, submodule
update) are still defined as synchronous `fn`, so they run on the main thread and
freeze the whole UI for the **entire duration of the network operation** —
seconds on a slow link, minutes on a large clone. `fetch_background` is worse: a
UI poller calls it on a timer, so the app freezes periodically on its own.

The codebase already knows the fix and applies it elsewhere. `watch_repo`
(`src-tauri/src/lib.rs:898-911`) and `launch_difftool`/`launch_mergetool`
(`:655-674`) were deliberately made `async fn` with comments explaining that a
synchronous command "would freeze the whole app." This plan extends that same,
already-blessed pattern to the network commands that were missed.

## Current state

- `src-tauri/src/lib.rs` — all Tauri command handlers. Commands are thin wrappers
  that call a method on `engine: State<GixEngine>` (the managed `lady_git`
  engine) and map the error to a string. The engine methods are blocking.

- The **already-async exemplar** to match — note `async fn` and the `State<'_, …>`
  (explicit-lifetime) form of every `State` param (`src-tauri/src/lib.rs:898-911`):

  ```rust
  // Async so the watch registration (which can block while the OS walks the tree
  // to install per-directory watches) runs off the main thread — a synchronous
  // command would freeze the UI for seconds on a large repo.
  #[tauri::command]
  async fn watch_repo(
      repo: RepoId,
      app: tauri::AppHandle,
      engine: State<'_, GixEngine>,
      watchers: State<'_, watcher::RepoWatchers>,
  ) -> Result<(), String> {
      let workdir = engine.workdir_path(&repo).map_err(|e| e.to_string())?;
      ...
  }
  ```

- The commands to convert, **as they exist today** (all synchronous `fn`, all on
  the main thread):

  - `clone_repo` (`src-tauri/src/lib.rs:785-845`) — spawns `git clone`, reads its
    stderr **line by line in a blocking loop**, then `child.wait()`. Freezes the
    UI for the whole clone:
    ```rust
    #[tauri::command]
    fn clone_repo(
        url: String,
        dest: String,
        account: Option<String>,
        app: tauri::AppHandle,
        engine: State<GixEngine>,
    ) -> Result<RepoId, String> {
    ```
  - `fetch` (`:850-871`):
    ```rust
    #[tauri::command]
    fn fetch(
        repo: RepoId,
        remote: Option<String>,
        app: tauri::AppHandle,
        engine: State<GixEngine>,
        hosting: State<'_, Hosting>,
    ) -> Result<(), String> {
    ```
  - `fetch_background` (`:876-892`):
    ```rust
    #[tauri::command]
    fn fetch_background(
        repo: RepoId,
        engine: State<GixEngine>,
        hosting: State<'_, Hosting>,
    ) -> Result<(), String> {
    ```
  - `pull` (`:927-955`):
    ```rust
    #[tauri::command]
    fn pull(
        repo: RepoId,
        remote: Option<String>,
        branch: Option<String>,
        app: tauri::AppHandle,
        engine: State<GixEngine>,
        hosting: State<'_, Hosting>,
    ) -> Result<(), String> {
    ```
  - `push` (`:960-...`) — has `#[allow(clippy::too_many_arguments)]` between the
    command attribute and the fn; keep that attribute:
    ```rust
    #[tauri::command]
    #[allow(clippy::too_many_arguments)]
    fn push(
        repo: RepoId,
        remote: Option<String>,
        branch: Option<String>,
        set_upstream: bool,
        force: bool,
        app: tauri::AppHandle,
        engine: State<GixEngine>,
        hosting: State<'_, Hosting>,
    ) -> Result<(), String> {
    ```
  - `init_submodules` (`:511-514`), `update_submodules` (`:517-520`),
    `sync_submodules` (`:522-526`) — submodule update fetches over the network:
    ```rust
    #[tauri::command]
    fn update_submodules(repo: RepoId, engine: State<GixEngine>) -> Result<(), String> {
        engine.update_submodules(&repo).map_err(|e| e.to_string())
    }
    ```

- **Key facts that make this conversion safe**:
  - `GixEngine` and `Hosting` are registered with `.manage(...)` in `run()`
    (`src-tauri/src/lib.rs:2494-2496`). Tauri requires managed state to be
    `Send + Sync + 'static`, so they already satisfy the bounds an async command
    needs. `watch_repo` already borrows `State<'_, GixEngine>` async — proof it
    compiles.
  - None of these command bodies contain an `.await`. They stay byte-for-byte
    identical; only the function signature changes. Tauri runs an async command
    via `async_runtime::spawn`, i.e. on a worker thread, not the main thread.
    The blocking body simply occupies that worker for its duration — which is
    exactly what `watch_repo` already does.
  - `tauri::generate_handler![...]` (`:2501`) handles sync and async commands
    identically. **No change to the handler registration list is needed.**

## Commands you will need

| Purpose   | Command                                                        | Expected on success         |
|-----------|---------------------------------------------------------------|-----------------------------|
| Compile   | `cargo build -p lady-app`                                      | exit 0, no errors           |
| Lint      | `cargo clippy -p lady-app --all-targets -- -D warnings`        | exit 0, no warnings         |
| Format    | `cargo fmt --all -- --check`                                   | exit 0                      |
| Tests     | `cargo test -p lady-app`                                       | all pass                    |

(Commands taken from `AGENTS.md` "Canonical Verification Gates". This change is
backend-only Rust; the UI build (`npm --prefix ui run build`) is unaffected and
need not be run.)

## Scope

**In scope** (the only file you should modify):
- `src-tauri/src/lib.rs`

**Out of scope** (do NOT touch):
- `crates/lady-git/src/lib.rs` and any other crate — the engine methods stay
  blocking; we only change which thread calls them.
- Any UI file under `ui/` — the frontend already calls these via `await invoke(...)`
  (e.g. `ui/src/branchActions.ts:174,184,194`), so it needs no change.
- The `tauri::generate_handler![...]` list (`:2501-2660`) — registration is
  unchanged for async commands.
- Cheap commands not listed in this plan (e.g. `load_settings`, `app_info`,
  `deinit_submodule`) — they are sub-millisecond; converting them adds churn for
  no benefit. Leave them synchronous.

## Git workflow

- Branch: `advisor/009-async-network-git-commands`
- One commit for the whole plan is fine (single cohesive change). Message style
  is Conventional Commits (see `git log --oneline`): e.g.
  `perf(commands): run network git commands off the main thread`.
- Do NOT push or open a PR unless the operator instructed it.

## Steps

For **each** command listed below, apply the identical transformation:

1. Insert the `async` keyword before `fn` (after any attributes like
   `#[allow(...)]`).
2. Change **every** `State<T>` parameter to `State<'_, T>`. Params already
   written as `State<'_, Hosting>` stay as-is — only the elided
   `State<GixEngine>` (and any other elided `State<...>`) gains the `'_`.
3. Leave the function body completely unchanged.

### Step 1: Convert the five primary network commands

Convert, in `src-tauri/src/lib.rs`:
- `clone_repo` (`:786`) → `async fn clone_repo`, `engine: State<'_, GixEngine>`.
- `fetch` (`:851`) → `async fn fetch`, `engine: State<'_, GixEngine>`
  (`hosting` already has `'_`).
- `fetch_background` (`:877`) → `async fn fetch_background`,
  `engine: State<'_, GixEngine>`.
- `pull` (`:928`) → `async fn pull`, `engine: State<'_, GixEngine>`.
- `push` (`:962`) → keep `#[allow(clippy::too_many_arguments)]`, then
  `async fn push`, `engine: State<'_, GixEngine>`.

**Verify**: `cargo build -p lady-app` → exit 0. If the compiler complains about a
`State` lifetime (e.g. "implementation is not general enough" or a lifetime on
`State`), you missed adding `'_` to a `State` param on that function — fix and
rebuild.

### Step 2: Convert the three submodule network commands

Convert `init_submodules` (`:512`), `update_submodules` (`:518`),
`sync_submodules` (`:523`) the same way → `async fn`, `engine: State<'_, GixEngine>`.

**Verify**: `cargo build -p lady-app` → exit 0.

### Step 3: Lint, format, test

**Verify**:
- `cargo clippy -p lady-app --all-targets -- -D warnings` → exit 0, no warnings.
- `cargo fmt --all -- --check` → exit 0 (run `cargo fmt --all` if it reports
  diffs, then re-check).
- `cargo test -p lady-app` → all pass.

### Step 4: Confirm the conversion is complete

**Verify**: each command below now reads `async fn`:
```
grep -nE 'async fn (clone_repo|fetch|fetch_background|pull|push|init_submodules|update_submodules|sync_submodules)\b' src-tauri/src/lib.rs
```
Expected: 8 matching lines.

## Test plan

No new automated tests. These commands are thin pass-throughs to the blocking
`lady_git` engine; their behavior (inputs, outputs, errors) is **unchanged** —
only the thread they execute on changes, which Rust's type system cannot assert
and which existing tests do not cover. Correctness is verified by:

- Compilation + clippy + the existing `cargo test -p lady-app` suite passing
  (proves the signatures are valid async commands and nothing regressed).
- The `grep` in Step 4 (proves every target command is now async).

Manual confirmation (optional, recommended for the reviewer, not required to pass
the plan): start the app against a repo with a slow/large remote, click Fetch,
and confirm the window stays responsive (you can scroll the commit graph) while
the fetch runs — previously it froze.

## Done criteria

ALL must hold:

- [ ] `cargo build -p lady-app` exits 0
- [ ] `cargo clippy -p lady-app --all-targets -- -D warnings` exits 0
- [ ] `cargo fmt --all -- --check` exits 0
- [ ] `cargo test -p lady-app` passes
- [ ] The Step 4 grep returns 8 `async fn` lines
- [ ] Only `src-tauri/src/lib.rs` is modified (`git status`)
- [ ] `plans/README.md` status row for 009 updated

## STOP conditions

Stop and report back (do not improvise) if:

- The drift check shows `src-tauri/src/lib.rs` changed since `9666d42` and a
  target command no longer matches the excerpt above.
- After adding `async` and the `State<'_, …>` lifetimes, the build fails with an
  error you cannot resolve by adding a missing `'_` to a `State` param (e.g. an
  engine method is `!Send`, or a non-`Send` value is held across the body). This
  would mean the simple async-fn pattern is insufficient and the command needs
  `tauri::async_runtime::spawn_blocking` instead — report it rather than guessing.
- Converting a command appears to require touching any file outside
  `src-tauri/src/lib.rs`.

## Maintenance notes

- For the human/agent who owns this next: plain `async fn` with a blocking body
  moves work off the **main thread** but still occupies a Tauri async-runtime
  **worker thread** for the call's duration. That is fine at Lady's concurrency
  (a user runs few network ops at once) and matches the existing `watch_repo`
  convention. If profiling ever shows worker-pool starvation under heavy
  concurrent use, the more rigorous fix is to wrap the blocking engine call in
  `tauri::async_runtime::spawn_blocking(move || ...)`, which requires cloning the
  needed handles out of `State` first (the `State` guard is not `'static`).
- A reviewer should check that no `State<T>` param on a converted command was
  left without the `'_` lifetime, and that no `.await` was accidentally
  introduced into a body.
- Plans 010 (heavy local reads) and 011 (mutating commands) apply the same
  pattern to the remaining blocking commands and can be done in any order
  relative to this one.
