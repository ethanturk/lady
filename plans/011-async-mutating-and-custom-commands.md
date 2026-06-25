# Plan 011: Move mutating and custom-command handlers off the Tauri main thread

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 6340849..HEAD -- src-tauri/src/lib.rs`
> If `src-tauri/src/lib.rs` changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none (independent of 009 and 010; same conversion pattern)
- **Category**: perf
- **Planned at**: commit `6340849`, 2026-06-25

## Why this matters

A synchronous `#[tauri::command] fn` runs **on the main (UI) thread** in Tauri, so
blocking work freezes the WebView until it returns (Tauri docs: *"Commands
without the async keyword are executed on the main thread"*).

Two groups of blocking commands still run synchronously on the main thread:

- **`run_custom_command`** executes an **arbitrary, user-defined** command
  against the repo and waits for it to finish. The user can configure anything —
  `npm install`, a test run, a long script — so this can freeze the entire UI for
  an unbounded amount of time.
- **History-rewriting / integration commands** — `merge`, `rebase`,
  `cherry_pick`, `revert` — shell out to git and can take seconds on a large
  repo, freezing the UI for the duration.

The codebase already made `launch_difftool`/`launch_mergetool` async with this
exact rationale — *"running it on the main thread would freeze the whole app for
the tool's lifetime"* (`src-tauri/src/lib.rs:651-654`). This plan extends that to
the mutating and custom-command handlers.

## Current state

- `src-tauri/src/lib.rs` — Tauri command handlers; thin wrappers over
  `engine: State<GixEngine>` (managed, **blocking** `lady_git` engine).

- **Already-async exemplar to match** (`src-tauri/src/lib.rs:655-...`):
  ```rust
  // ... running it on the main thread would freeze the whole app ...
  #[tauri::command]
  async fn launch_difftool(
      repo: RepoId,
      ...
  ```

- The commands to convert, **as they exist today** (all synchronous `fn`):

  - `run_custom_command` (`:644-653`):
    ```rust
    #[tauri::command]
    fn run_custom_command(
        repo: RepoId,
        template: String,
        values: std::collections::HashMap<String, String>,
        engine: State<GixEngine>,
    ) -> Result<lady_proto::CommandOutput, String> {
        let argv = lady_git::custom::build_argv(&template, &values);
        engine.run_custom(&repo, &argv).map_err(|e| e.to_string())
    }
    ```
  - `merge` (`:1085-1107`):
    ```rust
    #[tauri::command]
    fn merge(
        repo: RepoId,
        source: String,
        fast_forward: String,
        commit_message: Option<String>,
        engine: State<GixEngine>,
    ) -> Result<MergeOutcome, String> {
    ```
  - `cherry_pick` (`:1113-1122`):
    ```rust
    #[tauri::command]
    fn cherry_pick(
        repo: RepoId,
        oid: String,
        engine: State<GixEngine>,
    ) -> Result<ApplyOutcome, String> {
    ```
  - `revert` (`:1125-...`):
    ```rust
    #[tauri::command]
    fn revert(repo: RepoId, oid: String, engine: State<GixEngine>) -> Result<ApplyOutcome, String> {
    ```
  - `rebase` (`:1136-1146`):
    ```rust
    #[tauri::command]
    fn rebase(
        repo: RepoId,
        branch: String,
        onto: String,
        engine: State<GixEngine>,
    ) -> Result<RebaseOutcome, String> {
    ```

- **Why this is safe** (identical reasoning to plans 009/010): `GixEngine` is
  `.manage()`d (already `Send + Sync + 'static`); `watch_repo`/`launch_difftool`
  already use it in async commands; none of the bodies below contain `.await`, so
  they stay byte-for-byte identical; `tauri::generate_handler!` handles async and
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
- Any crate under `crates/` — `lady_git::custom::build_argv` and the engine
  methods stay as-is; we only change the calling thread.
- Any UI file under `ui/` — callers already `await invoke(...)`.
- The `tauri::generate_handler![...]` list — unchanged for async commands.
- The `*_abort` / `*_continue` / `*_skip` companions (`merge_abort`,
  `rebase_abort`, `sequencer_abort`, `rebase_continue`, `rebase_skip`) and
  `rebase_interactive` / `rebase_range`. They are fast or out of this plan's
  focus; leave them synchronous to keep the change tightly scoped. (A follow-up
  may convert `rebase_interactive`/`rebase_range` if profiling shows they block;
  noted in Maintenance.)
- Network commands (plan 009) and heavy read commands (plan 010).

## Git workflow

- Branch: `advisor/011-async-mutating-and-custom-commands`
- One commit is fine. Conventional Commits message, e.g.
  `perf(commands): run mutating and custom commands off the main thread`.
- Do NOT push or open a PR unless instructed.

## Steps

For **each** command below: insert `async` before `fn`, change every `State<T>`
param to `State<'_, T>`, leave the body unchanged.

### Step 1: Convert `run_custom_command`

Convert `run_custom_command` (`:645`) → `async fn`, `engine: State<'_, GixEngine>`.

**Verify**: `cargo build -p lady-app` → exit 0.

### Step 2: Convert the integration/history commands

Convert `merge` (`:1086`), `cherry_pick` (`:1114`), `revert` (`:1125`),
`rebase` (`:1137`) the same way.

**Verify**: `cargo build -p lady-app` → exit 0.

### Step 3: Lint, format, test

**Verify**:
- `cargo clippy -p lady-app --all-targets -- -D warnings` → exit 0.
- `cargo fmt --all -- --check` → exit 0 (run `cargo fmt --all` first if needed).
- `cargo test -p lady-app` → all pass.

### Step 4: Confirm completeness

**Verify**:
```
grep -nE 'async fn (run_custom_command|merge|cherry_pick|revert|rebase)\b' src-tauri/src/lib.rs
```
Expected: exactly 5 matching lines. (`rebase` must match `async fn rebase` only —
confirm `rebase_abort`/`rebase_continue`/`rebase_skip`/`rebase_interactive`/
`rebase_range` are **not** in the result; the `\b` after `rebase` ensures that.)

## Test plan

No new automated tests — unchanged pass-throughs to `lady_git`; only the
execution thread changes. Verification is compilation + clippy + the existing
`cargo test -p lady-app` suite + the Step 4 grep.

Optional manual check for the reviewer: configure a custom command that sleeps a
few seconds, run it, and confirm the UI stays responsive (scroll the graph) while
it runs — previously it froze.

## Done criteria

ALL must hold:

- [ ] `cargo build -p lady-app` exits 0
- [ ] `cargo clippy -p lady-app --all-targets -- -D warnings` exits 0
- [ ] `cargo fmt --all -- --check` exits 0
- [ ] `cargo test -p lady-app` passes
- [ ] The Step 4 grep returns exactly 5 `async fn` lines and none of the
      `rebase_*` companions
- [ ] Only `src-tauri/src/lib.rs` is modified (`git status`)
- [ ] `plans/README.md` status row for 011 updated

## STOP conditions

Stop and report back (do not improvise) if:

- The drift check shows `src-tauri/src/lib.rs` changed since `6340849` and a
  target command no longer matches its excerpt.
- The build fails with an error not resolved by adding a missing `'_` to a
  `State` param (non-`Send` value held across the body, or a `!Send` engine
  method). That means the command needs `spawn_blocking` — report it.
- The change appears to require editing any file outside `src-tauri/src/lib.rs`.

## Maintenance notes

- Same trade-off as plans 009/010: `async fn` with a blocking body runs on an
  async worker thread, not the main thread. For genuinely long custom commands,
  the deeper fix is `tauri::async_runtime::spawn_blocking` so the worker pool is
  never tied up — defer unless profiling shows a problem.
- Deferred follow-up: `rebase_interactive` (`:1234`) and `rebase_range` (`:1266`)
  can also block; convert them with this same pattern if they show up in UI-freeze
  reports.
- A reviewer should confirm every converted command kept all `State` params as
  `State<'_, T>`, that no `.await` was added, and that the `rebase_*` companions
  were left untouched.
