# Changelog

All notable changes to Lady. Format follows [Keep a Changelog](https://keepachangelog.com);
this project uses semantic-ish version lines per phase.

## [0.0.10] — Settings organization

### Changed
- Split the Settings dialog navigation into **Global** and **Repository**
  sections.
- Moved repository-specific Git overrides, identity, and credential controls out
  of the global Git defaults pane.

## [1.3.0] — GA ship (Phase 6 — Polish & ship)

The general-availability release: the feature-complete app (Core Parity + Fast-follow
+ AI) made into a signed, auto-updating, accessible, performance-budgeted product.

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

## [1.2.0] — AI (Phase 5) — GitKraken Git AI parity + superset

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

## [1.1.0] — Fast-follow (Phase 4) — hosting + niche

### Added
- Forge-agnostic hosting trait; **GitLab / Bitbucket / Azure DevOps** auth +
  PR/MR creation; create-remote-repo for all four forges; GitHub notifications
  inbox.
- **Git LFS**, **git-flow**, and submodule management (including nested).

## [1.0.0] — Core Parity (Phases 1–3) — first public release

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

[1.3.0]: https://github.com/ethanturk/lady/releases/tag/v1.3.0
[0.0.10]: https://github.com/ethanturk/lady/releases/tag/v0.0.10
[1.2.0]: https://github.com/ethanturk/lady/releases/tag/v1.2.0
[1.1.0]: https://github.com/ethanturk/lady/releases/tag/v1.1.0
[1.0.0]: https://github.com/ethanturk/lady/releases/tag/v1.0.0
