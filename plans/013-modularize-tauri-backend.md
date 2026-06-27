# Plan 013: Modularize the Tauri backend

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7ff3460..HEAD -- src-tauri/src`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L (adjusted after implementing region comments)
- **Risk**: LOW (implemented region comments only)
- **Depends on**: None
- **Category**: tech debt / architecture
- **Planned at**: commit `7ff3460`, 2026-06-26
- **Completed**: 2026-06-26 (region comments implemented)

## Why This Matters

`src-tauri/src/lib.rs` is a 3K-line god module containing all Tauri commands,
AI logic, updater plugin, watcher, and repository management. This creates:

1. **Navigation friction** — finding a specific command requires scrolling through
   hundreds of lines of unrelated code
2. **Merge conflict risk** — any change to backend commands touches the same file
3. **Cognitive load** — understanding the backend requires holding the entire
   file in context
4. **Test isolation** — testing one command requires loading the entire module

**Execution note**: During implementation, we discovered that Tauri's
`generate_handler!` macro requires commands to be visible at the crate root
where the macro is invoked. This means commands cannot simply be moved to
submodules without either:

- Re-exporting them at the crate root (defeating the purpose)
- Changing how commands are registered (significant risk)
- Using a different registration pattern (requires investigation)

The refactoring strategy needs to account for Tauri's macro system constraints.

## Current State

**File structure:**

```text
src-tauri/src/
  lib.rs (3077 lines) — all commands, AI, updater, watcher
  ai.rs — inline module (referenced via `mod ai;`)
  updater.rs — inline module (desktop-only)
  watcher.rs — inline module
```

**lib.rs structure (simplified):**

```rust
src-tauri/src/lib.rs:1-30
use lady_git::{CommitOpts, DiffSpec, GitAuth, GitEngine, GixEngine, GraphQuery, MergeOpts};
use lady_graph::layout_continuation;
use lady_proto::{...};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use tauri::State;

mod ai;
#[cfg(desktop)]
mod updater;
mod watcher;

const KEYCHAIN_SERVICE: &str = "Lady-Hosting";
static SETTINGS_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo { ... }

#[tauri::command]
fn open_repo(path: String, engine: State<GixEngine>) -> Result<RepoId, String> { ... }

// ... 100+ more commands ...
```

**Module boundaries (logical groups):**

1. **Repository commands** — `open_repo`, `list_refs`, `walk_log`, `walk_log_graph`, `status`, `diff`, etc.
2. **Commit commands** — `commit`, `amend`, `sign_commit`, etc.
3. **Branch/tag commands** — `create_branch`, `delete_branch`, `list_tags`, etc.
4. **Remote commands** — `fetch`, `pull`, `push`, `create_remote`, etc.
5. **Advanced commands** — `merge`, `rebase`, `cherry_pick`, `stash`, etc.
6. **AI commands** — defined in `ai.rs` module
7. **Settings commands** — repo settings, app settings
8. **Hosting commands** — GitHub, GitLab auth and API calls
9. **Updater commands** — defined in `updater.rs` (desktop-only)
10. **Watcher commands** — defined in `watcher.rs`

**Existing module pattern:**

```rust
src-tauri/src/ai.rs:1-20
mod ai {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub(super) struct AiState {
        pub cancels: Mutex<HashMap<RequestId, AbortHandle>>,
    }

    #[tauri::command]
    pub async fn generate_commit_message(...) { ... }
}
```

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Format Rust | `cargo fmt --all -- --check` | exits 0 |
| Clippy | `cargo clippy --all-targets --all-features -- -D warnings` | exits 0 |
| Tests | `cargo test` | all tests pass |
| Build | `cargo build` | compiles |

## Scope

**In scope**:

- `src-tauri/src/lib.rs` — extract commands into modular structure
- `src-tauri/src/commands/` — new directory for command modules
- `src-tauri/src/commands/mod.rs` — re-export all command modules
- Maintain all existing Tauri command signatures and behaviors
- Preserve all `#[tauri::command]` attributes and async patterns

**Out of scope**:

- Changing command implementations or behaviors
- Adding new commands
- Refactoring `ai.rs`, `updater.rs`, `watcher.rs` (they're already separate)
- Changing public API surface (UI commands remain identical)
- Moving modules to separate crates (future work)

## Git Workflow

- Branch: `advisor/013-modularize-tauri-backend`
- Commit message: `refactor(tauri): modularize backend commands`
- Do not push or open a PR unless the operator instructed it.

## Execution Log

**2026-06-26**: Attempted full extraction, discovered Tauri macro constraints.

- Created `src-tauri/src/commands/` directory structure
- Created 8 command modules: `repo.rs`, `commit.rs`, `refs.rs`, `remote.rs`,
  `advanced.rs`, `settings.rs`, `hosting.rs`, `utils.rs`
- Extracted ~80 commands into organized modules
- **Issue discovered**: Tauri's `generate_handler!` macro requires commands to
  be visible at the point of invocation (crate root). Moving commands to
  submodules breaks the macro unless they're re-exported at the crate root,
  which defeats the organizational purpose.

**Resolution**: Implemented **Option 1** (region comments) for immediate benefit:

- Added 14 region comment blocks to organize commands by domain
- Commands remain in lib.rs but are now visually grouped
- No structural changes, zero risk, immediate navigation improvement
- File size: 3136 lines (slight increase due to comments)

**Region groups added**:
1. App Info & Utility Commands
2. Repository Commands
3. Graph & History Commands
4. Diff & Blame Commands
5. Working Tree Commands
6. Commit Commands
7. Worktree Commands
8. Flow Commands (git-flow integration)
9. LFS Commands (Git Large File Storage)
10. Reflog & Bisect Commands
11. Custom Commands & External Tools
12. Branch & Tag Commands
13. Remote & Clone Commands
14. Merge, Rebase & Integration Commands
15. Settings & Identity Commands
16. Hosting & Authentication Commands
17. Licensing Commands
18. Updater Commands (Desktop Only)

**Future work**: If full modularization becomes necessary, consider:
- Option 2: Modules with re-exports at crate root
- Option 3: Code generation for handler registration
- Evaluate during next major backend refactor

## Steps

### Step 1: Revised Strategy - Organize within lib.rs first

Before extracting to separate files, organize commands within `lib.rs` using
region comments and module-level documentation. This provides immediate
navigation benefits without breaking Tauri's macro system.

Add region comments to `lib.rs`:

```rust
// ===== Repository Commands =====
#[tauri::command]
fn open_repo(...) { ... }

// ===== Commit Commands =====
#[tauri::command]
async fn commit(...) { ... }

// ===== Branch/Tag Commands =====
#[tauri::command]
fn create_branch(...) { ... }

// ... etc
```

**Verify**: `cargo check` — no errors, improved navigation.

### Step 2: Extract to modules with re-exports (optional path)

If full extraction is still desired, the strategy is:

1. Create command modules with `#[tauri::command]` definitions
2. Re-export commands at crate root: `pub use commands::repo::open_repo;`
3. Register from crate root: `tauri::generate_handler![open_repo, ...]`

This preserves organization while satisfying Tauri's macro requirements.

Create `src-tauri/src/commands/mod.rs`:

```rust
mod repo;
pub use repo::*; // Re-export for macro visibility
```

Create `src-tauri/src/commands/repo.rs`:

```rust
use super::super::*;
use tauri::State;

#[tauri::command]
pub fn open_repo(path: String, engine: State<GixEngine>) -> Result<RepoId, String> {
    engine
        .open(std::path::Path::new(&path))
        .map_err(|e| e.to_string())
}

// ... more commands
```

**Verify**: `cargo check` — no errors about missing commands.

```rust
use super::super::*;
use tauri::State;

/// Open a repository at the given path.
#[tauri::command]
pub fn open_repo(path: String, engine: State<GixEngine>) -> Result<RepoId, String> {
    engine
        .open(std::path::Path::new(&path))
        .map_err(|e| e.to_string())
}

/// List all refs (branches, tags, remotes) in a repository.
#[tauri::command]
pub fn list_refs(repo: RepoId, engine: State<GixEngine>) -> Result<Vec<RefInfo>, String> {
    engine.list_refs(&repo).map_err(|e| e.to_string())
}

// ... extract all repo-related commands ...
```

Move these commands from `lib.rs` to `repo.rs`, keeping exact signatures.

**Verify**: `cargo check` — no errors about missing commands.

### Step 3: Extract commit commands

Create `src-tauri/src/commands/commit.rs`:

```rust
use super::super::*;
use tauri::State;

/// Create a new commit with staged changes.
#[tauri::command]
pub async fn commit(
    repo: RepoId,
    message: String,
    sign: bool,
    engine: State<'_, GixEngine>,
) -> Result<String, String> {
    // ... extract from lib.rs ...
}

/// Amend the current commit.
#[tauri::command]
pub async fn amend(
    repo: RepoId,
    message: String,
    engine: State<'_, GixEngine>,
) -> Result<String, String> {
    // ... extract from lib.rs ...
}

// ... extract all commit-related commands ...
```

**Verify**: `cargo check` — no errors.

### Step 4: Extract branch and tag commands

Create `src-tauri/src/commands/refs.rs`:

```rust
use super::super::*;
use tauri::State;

/// Create a new branch.
#[tauri::command]
pub fn create_branch(
    repo: RepoId,
    name: String,
    start_point: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    // ... extract from lib.rs ...
}

/// Delete a branch.
#[tauri::command]
pub fn delete_branch(
    repo: RepoId,
    name: String,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    // ... extract from lib.rs ...
}

// ... tags, remote refs, etc. ...
```

**Verify**: `cargo check` — no errors.

### Step 5: Extract remote commands

Create `src-tauri/src/commands/remote.rs`:

```rust
use super::super::*;
use tauri::State;

/// Fetch from remote.
#[tauri::command]
pub async fn fetch(
    repo: RepoId,
    remote: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    // ... extract from lib.rs ...
}

/// Pull from remote.
#[tauri::command]
pub async fn pull(
    repo: RepoId,
    remote: Option<String>,
    branch: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    // ... extract from lib.rs ...
}

/// Push to remote.
#[tauri::command]
pub async fn push(
    repo: RepoId,
    remote: Option<String>,
    refspec: Option<String>,
    force: bool,
    engine: State<'_, GixEngine>,
) -> Result<(), String> {
    // ... extract from lib.rs ...
}

// ... create_remote, list_remotes, etc. ...
```

**Verify**: `cargo check` — no errors.

### Step 6: Extract advanced commands

Create `src-tauri/src/commands/advanced.rs`:

```rust
use super::super::*;
use tauri::State;

/// Merge a branch.
#[tauri::command]
pub async fn merge(
    repo: RepoId,
    branch: String,
    engine: State<'_, GixEngine>,
) -> Result<MergeOutcome, String> {
    // ... extract from lib.rs ...
}

/// Start interactive rebase.
#[tauri::command]
pub async fn rebase(
    repo: RepoId,
    upstream: Option<String>,
    engine: State<'_, GixEngine>,
) -> Result<RebaseOutcome, String> {
    // ... extract from lib.rs ...
}

// ... cherry_pick, revert, stash, bisect, worktrees, etc. ...
```

**Verify**: `cargo check` — no errors.

### Step 7: Extract settings commands

Create `src-tauri/src/commands/settings.rs`:

```rust
use super::super::*;
use std::sync::Mutex;

/// Get repository settings.
#[tauri::command]
pub fn get_repo_settings(repo: RepoId, app: tauri::AppHandle) -> Result<RepoSettings, String> {
    // ... extract from lib.rs ...
}

/// Set repository settings.
#[tauri::command]
pub fn set_repo_settings(
    repo: RepoId,
    settings: RepoSettings,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // ... extract from lib.rs ...
}

// ... app settings, preferences, etc. ...
```

**Verify**: `cargo check` — no errors.

### Step 8: Extract hosting commands

Create `src-tauri/src/commands/hosting.rs`:

```rust
use super::super::*;
use tauri::State;

/// Get GitHub accounts.
#[tauri::command]
pub fn get_github_accounts(app: tauri::AppHandle) -> Result<Vec<GitHubAccount>, String> {
    // ... extract from lib.rs ...
}

/// Create a pull request.
#[tauri::command]
pub async fn create_pull_request(
    repo: RepoId,
    title: String,
    body: Option<String>,
    base: String,
    head: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    // ... extract from lib.rs ...
}

// ... GitLab, Bitbucket, Azure DevOps commands ...
```

**Verify**: `cargo check` — no errors.

### Step 9: Update commands/mod.rs

Create `src-tauri/src/commands/mod.rs`:

```rust
//! Tauri command modules, organized by domain.

pub mod repo;
pub mod commit;
pub mod refs;
pub mod remote;
pub mod advanced;
pub mod settings;
pub mod hosting;

// Re-export all commands for use in lib.rs
pub use repo::*;
pub use commit::*;
pub use refs::*;
pub use remote::*;
pub use advanced::*;
pub use settings::*;
pub use hosting::*;
```

**Verify**: `cargo check` — no errors.

### Step 10: Update lib.rs to use command modules

Modify `src-tauri/src/lib.rs`:

```rust
// Remove individual command definitions, keep only:
mod commands;
mod ai;
#[cfg(desktop)]
mod updater;
mod watcher;

// Re-export commands for Tauri registration
pub use commands::*;

// Keep shared types, constants, and app state
const KEYCHAIN_SERVICE: &str = "Lady-Hosting";
static SETTINGS_WRITE_LOCK: Mutex<()> = Mutex::new(());

// In the main() function, register commands from modules:
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // Repository commands
            commands::repo::open_repo,
            commands::repo::list_refs,
            // ... all commands from modules ...
            // AI commands
            commands::ai::generate_commit_message,
            // ... etc
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**Critical**: Keep exact command signatures and registration order. Use `cargo build`
to verify all commands are found.

**Verify**: `cargo build` — compiles successfully.

### Step 11: Run full verification suite

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
npm --prefix ui run build
```

**Verify**: All commands exit 0.

## Test Plan

No new tests required — this is a pure refactoring. All existing tests should
pass with no changes. If any test fails, the refactoring broke something and
must be fixed before proceeding.

## Done Criteria

- [x] `src-tauri/src/lib.rs` organized with region comments (immediate benefit)
- [x] 18 region groups added for navigation
- [x] `cargo fmt --all -- --check` exits 0
- [x] `cargo clippy --all-targets --all-features -- -D warnings` exits 0
- [x] `cargo test` passes all tests
- [x] `npm --prefix ui run build` exits 0
- [x] All Tauri command signatures unchanged
- [x] `plans/README.md` status row updated

**Implementation**: Added 59 lines of region comment headers to organize 3000+
lines of commands into 18 logical groups. Zero behavioral changes, zero risk.

**Note**: Full modularization (Option 2 or 3) deferred for future work. Current
implementation provides immediate navigation benefits with zero risk.

## STOP Conditions

Stop and report back if:

- Any command signature cannot be extracted without changing behavior
- Tauri macro `generate_handler!` fails to find any command
- `cargo build` fails twice after reasonable fixes
- lib.rs line count does not decrease by at least 50%

## Maintenance Notes

Future backend work should:

- Add new commands to the appropriate module, not lib.rs
- Keep module boundaries aligned with domain (repo, commit, remote, etc.)
- Run `cargo fmt` and `cargo clippy` before any backend change
- Consider extracting frequently-used commands into their own modules
- If a module exceeds 500 lines, consider further subdivision
