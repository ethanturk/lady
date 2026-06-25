# Plan 007: Serialize settings writes through one backend path

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- src-tauri/src/lib.rs src-tauri/src/ai.rs crates/lady-proto/src/lib.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: correctness
- **Planned at**: commit `9666d42`, 2026-06-24

## Why This Matters

Several Tauri commands read the whole settings file, mutate one section, and
write the whole file back. The code already preserves some ownership boundaries
in `save_settings`, but separate commands can still race: for example, a repo
override update and a GitHub account update can both load old state and then
the later write can erase the earlier change. A small serialized update helper
keeps the simple TOML storage model while preventing lost updates inside one app
process.

## Current State

Relevant files:

- `src-tauri/src/lib.rs` - settings model, load/write helpers, Tauri commands.
- `src-tauri/src/ai.rs` - AI commands may also update settings; inspect before
  editing if it calls `load_settings_inner` or `write_settings`.
- `crates/lady-proto/src/lib.rs` - shared settings types if needed; likely
  out-of-scope unless tests reveal a type gap.

Excerpts:

```rust
src-tauri/src/lib.rs:1294-1308
pub(crate) fn load_settings_inner() -> Settings {
    settings_file()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn write_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_file()?;
    ...
    std::fs::write(&path, body).map_err(|e| e.to_string())
}
```

```rust
src-tauri/src/lib.rs:1317-1331
fn save_settings(mut settings: Settings) -> Result<(), String> {
    let on_disk = load_settings_inner();
    settings.license = on_disk.license;
    settings.ai = on_disk.ai;
    settings.ai_repos = on_disk.ai_repos;
    settings.defaults = on_disk.defaults;
    settings.repo_overrides = on_disk.repo_overrides;
    settings.github_accounts = on_disk.github_accounts;
    settings.auth_suggest_dismissed = on_disk.auth_suggest_dismissed;
    write_settings(&settings)
}
```

```rust
src-tauri/src/lib.rs:1407-1422
fn set_repo_override(...) -> Result<(), String> {
    let key = repo_settings_key(&repo, &engine)?;
    let mut s = load_settings_inner();
    ...
    write_settings(&s)
}
```

```rust
src-tauri/src/lib.rs:1559-1587
async fn add_github_account(...) -> Result<GitHubAccount, String> {
    ...
    let mut s = load_settings_inner();
    s.github_accounts.retain(|a| a.id != id);
    s.github_accounts.push(account.clone());
    write_settings(&s)?;
    Ok(account)
}
```

Existing tests near the bottom of `src-tauri/src/lib.rs` already cover settings
serde and repo overrides. Match that in-file test style.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Target tests | `cargo test -p lady-app settings` | settings-related tests pass |
| Full Rust tests | `cargo test` | all tests pass |
| Rust format | `cargo fmt --all -- --check` | exits 0 |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` | exits 0 |

## Scope

**In scope**:

- `src-tauri/src/lib.rs`
- `src-tauri/src/ai.rs` only if it performs settings writes that must use the
  new helper.
- Tests in the same files.

**Out of scope**:

- Migrating settings to SQLite.
- Changing the TOML schema.
- Changing keychain storage.
- Frontend settings UI redesign.
- Cross-process file locking unless you can add it with no new dependency and
  no broad rewrite. This plan targets lost updates inside the running app.

## Git Workflow

- Branch: `advisor/007-serialize-settings`
- Commit message: `fix(settings): serialize settings writes`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add a single update helper

In `src-tauri/src/lib.rs`, add a process-wide settings write lock. Keep it
simple, for example:

```rust
static SETTINGS_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn update_settings_inner(
    f: impl FnOnce(&mut Settings) -> Result<(), String>,
) -> Result<(), String> {
    let _guard = SETTINGS_WRITE_LOCK.lock().map_err(|_| "settings lock poisoned".to_string())?;
    let mut settings = load_settings_inner();
    f(&mut settings)?;
    write_settings(&settings)
}
```

If `static Mutex::new(())` is not accepted in the current Rust version, use
`std::sync::OnceLock<Mutex<()>>`.

**Verify**: `cargo test -p lady-app settings` -> existing settings tests still
pass.

### Step 2: Convert settings-writing commands

Replace load/mutate/write sequences with `update_settings_inner` in:

- `save_settings`
- `set_repo_override`
- `set_global_defaults`
- `add_github_account` after token validation/keychain write succeeds
- `remove_github_account`
- `assign_repo_account`
- `dismiss_repo_account_suggestion`
- any settings-writing helper in `src-tauri/src/ai.rs`

Preserve current ownership behavior in `save_settings`: recents/custom commands
from the UI must not clobber license, AI config, repo overrides, GitHub
accounts, or dismissed suggestions.

**Verify**: `cargo test -p lady-app settings` -> settings tests pass.

### Step 3: Add a regression test for lost-update behavior

Add a unit test near existing settings tests that exercises the update helper
directly:

- Start with a default settings value.
- Apply one update that writes `github_accounts` or `defaults`.
- Apply another update that writes `recent` or `repo_overrides`.
- Reload and assert both changes are present.

If current tests write to the user's real config path, do not add a flaky test.
Instead, refactor the helper minimally so tests can inject a temp settings path,
or STOP if that requires a broad rewrite.

**Verify**: `cargo test -p lady-app settings` -> includes the new regression
test and passes.

### Step 4: Run full Rust gates

**Verify**: `cargo fmt --all -- --check` -> exits 0.

**Verify**: `cargo clippy --all-targets --all-features -- -D warnings` -> exits 0.

**Verify**: `cargo test` -> all tests pass.

## Test Plan

- Existing settings serde tests continue to pass.
- New regression test proves sequential update-helper calls preserve unrelated
  sections.
- If feasible, add a concurrency test spawning two threads that use the helper
  and assert both updates survive. Keep it deterministic; avoid sleeps.

## Done Criteria

- [ ] Settings-writing commands use one serialized update helper.
- [ ] `save_settings` still preserves fields owned by license, AI, repo settings,
  and GitHub account commands.
- [ ] New regression test covers unrelated settings updates not clobbering each
  other.
- [ ] `cargo fmt --all -- --check` exits 0.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` exits 0.
- [ ] `cargo test` exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report back if:

- `src-tauri/src/lib.rs:1294-1308` no longer matches the load/write helper
  shape.
- A correct fix requires a schema migration or replacing TOML storage.
- Tests would write to the user's real settings file and cannot be isolated with
  a small refactor.
- `src-tauri/src/ai.rs` owns settings writes in a way that cannot call the helper
  without creating a module cycle.

## Maintenance Notes

Future commands that mutate `Settings` must use the update helper. Reviewers
should flag new `let mut s = load_settings_inner(); ... write_settings(&s)`
patterns unless they are inside the helper itself.

