# Changelog

All notable changes to Lady. Format follows [Keep a Changelog](https://keepachangelog.com).
The current shipped app/UI release is `v0.0.15`; later semantic version sections
below are roadmap history, not published release tags.

## [0.0.15] — File multi-select in Local Changes

### Added
- Shift-click in Local Changes now selects contiguous file ranges across the
  visible unstaged/staged list order.
- Cmd/Ctrl-click now toggles individual files into multi-selection without
  expanding the range.

### Changed
- Stage/Unstage actions now operate on the current file multi-selection within
  the active staged/unstaged bucket, including keyboard shortcuts and file-row
  buttons.
- File-row context menus now use the current multi-selection when right-clicking
  a selected row, enabling bulk discard/ignore/stash/patch actions.

## [0.0.14] — Error handling polish & integration tests

### Changed
- Replaced all 22 production `unwrap()/expect()` calls with proper error handling:
  - Mutex poisoning recovery using `unwrap_or_else(|e| e.into_inner())`
  - Git process errors propagate with context
  - Infallible operations have enhanced panic messages
  - All verification gates pass

### Added
- 9 integration tests in `crates/lady-git/tests/integration/` covering:
  - Basic repo operations (init, status, commit, log, diff)
  - Branch creation and fast-forward merges
  - Merge conflict detection
  - Worktree management
  - Gix vs system git consistency

## [0.0.13] — Context menu hover affordance

### Fixed
- Context menu rows now show a hover background tint. The row button set an
  inline `background: transparent`, which beat the `.hov:hover` class rule and
  suppressed the tint; the inline override is dropped and `.hov` carries a
  transparent base instead. Disabled rows stay flat on hover.

## [0.0.12] — Pre-commit hook error dialog & diverged-pull reconcile

### Added
- A diverged `git pull` (local and remote both have new commits, no configured
  reconcile strategy) now opens a centered dialog to pick merge, rebase, or
  fast-forward-only — instead of failing with git's "Need to specify how to
  reconcile divergent branches" hint. The choice can be remembered per repo
  (persists `pull.rebase` / `pull.ff`).

### Changed
- Pre-commit (and other git hook) failures now surface in a dedicated, centered,
  scrollable dialog instead of being dumped inline into the Changes column,
  where the verbose multi-line output overflowed the file list. The dialog
  auto-opens on a new failure, is toggled from a top-bar alert icon, and clears
  on the next clean commit. The commit error path now also captures hook stdout
  (where the pre-commit framework writes its report) so the full output is kept.

## [0.0.11] — Responsive UI during git operations

### Changed
- Moved 22 blocking Tauri commands off the main thread by making them `async`,
  so the UI no longer freezes while a command runs. Covers network operations
  (clone, fetch, pull, push, submodule update/init/sync), repository-size-bound
  reads (commit-graph walk, diff, blame, file history, status, signature
  verification, reflog), and the mutating/custom commands (custom command run,
  merge, cherry-pick, revert, rebase).

## [0.0.10] — Settings organization

### Changed
- Split the Settings dialog navigation into **Global** and **Repository**
  sections.
- Moved repository-specific Git overrides, identity, and credential controls out
  of the global Git defaults pane.

## Roadmap history

### 1.3.0 target — GA ship (Phase 6 — Polish & ship)

The general-availability target: the feature-complete app (Core Parity +
Fast-follow + AI) made into a signed, auto-updating, accessible,
performance-budgeted product.

### Added
- **Packaging** via the Tauri bundler for macOS (`.app`/`.dmg`, Developer ID
  sign + notarize), Windows (`.msi`/NSIS, Authenticode), and Linux (AppImage +
  Flatpak). See [docs/PACKAGING.md](docs/PACKAGING.md).
- **Auto-update** via the Tauri updater plugin with **signed** manifests. In-app
  *Settings → Updates → Check for updates*; updates are explicit-action only and
  signature-verified against a committed public key (a tampered manifest is
  rejected — see `src-tauri/src/updater.rs` tests).
- **Performance benchmarks + budgets** (`criterion`) over a seeded offline
  synthetic fixture (`lady-fixtures`); a non-gating CI bench job; measured
  numbers in [docs/PERF.md](docs/PERF.md) — all inside the §8.9 budgets.
- **Accessibility**: full keyboard operability, focus-visible rings, ARIA
  roles/labels + live regions, WCAG AA contrast (light + dark), reduced-motion
  support. See [docs/ACCESSIBILITY.md](docs/ACCESSIBILITY.md) and
  [docs/KEYBOARD.md](docs/KEYBOARD.md).
- **Theming**: finalized CSS token system with an optional custom accent. See
  [docs/THEMING.md](docs/THEMING.md).
- **Docs**: user guide, AI/privacy, MCP setup, keyboard reference, this changelog.
- **Release CI** (`release.yml`): tag-triggered cross-platform build → green gate
  → sign/notarize → publish artifacts + signed update manifest.

### Changed
- Replaced hard-coded UI colors with semantic theme tokens across all views (only
  the intentional commit-graph lane palette remains literal).

### 1.2.0 target — AI (Phase 5) — GitKraken Git AI parity + superset

### Added
- BYOK provider abstraction + task model; local **Ollama** path; remote providers
  (OpenAI, Anthropic, Gemini, Azure, Mistral) — all wiremock-tested, never live.
- Explicit opt-in, per-repo toggle, first-use per-provider consent; best-effort
  secret **redaction** + token budgeting before remote sends (ADR-0008/0009).
- AI **commit messages**, **Commit Composer** (logical split), **explain**
  (commit/range/stash/working), **conflict resolution** (review-gated), **PR
  title/description**, **changelog**, **stash notes**.
- Read-only **MCP server** (`lady-mcp`) exposing repo context to external
  assistants. See [docs/MCP.md](docs/MCP.md).
- _Deferred:_ semantic commit search (optional stretch).

### 1.1.0 target — Fast-follow (Phase 4) — hosting + niche

### Added
- Forge-agnostic hosting trait; **GitLab / Bitbucket / Azure DevOps** auth +
  PR/MR creation; create-remote-repo for all four forges; GitHub notifications
  inbox.
- **Git LFS**, **git-flow**, and submodule management (including nested).

### 1.0.0 target — Core Parity (Phases 1–3) — first public-release target

### Added
- Commit **graph** (virtualized canvas), **diff viewer** (split/unified, syntax
  highlighting, image diffs), working & staged diffs.
- **Partial staging** (hunk + line), stage/unstage, discard; **commit/amend**.
- Branches & tags (create/delete/checkout); **fetch/pull/push** with system-git
  credentials; **stash**; **merge** (ff/no-ff), **cherry-pick**, **revert**;
  drag-&-drop merge/rebase.
- **3-pane merge conflict resolver**; **interactive rebase**; GPG + SSH commit
  **signing** with verification badges; **worktrees**; **reflog** (restore lost
  commits); **bisect**; custom commands; external diff/merge tools.
- **Blame**, **file history**, repository manager (open/clone/add, recent, tabs),
  **command palette**.
- GitHub auth + **pull request creation**; **licensing gate** (30-day trial +
  offline signed-key verification, ADR-0007).

[0.0.10]: https://github.com/ethanturk/lady/releases/tag/v0.0.10
