# Lady

A fast, friendly cross-platform **Git client** built in Rust — Fork-style daily
workflow with GitKraken-style AI. Native desktop app via [Tauri](https://tauri.app)
(Rust core + SolidJS WebView UI).

**Status:** GA — **v1.3.0**. Core Parity + Fast-follow + AI, now signed,
auto-updating, accessible (WCAG AA), and performance-budgeted across macOS,
Windows, and Linux. See [CHANGELOG.md](CHANGELOG.md).

## Features

- **Commit graph** (virtualized canvas), **diff viewer** (split/unified, syntax
  highlighting, image diffs), blame, file history.
- **Staging** down to the hunk and line; commit / amend with GPG + SSH signing.
- Branches & tags, fetch / pull / push, stash, merge, cherry-pick, revert,
  drag-&-drop merge/rebase.
- **3-pane conflict resolver**, interactive rebase, worktrees, reflog, bisect,
  custom commands, external diff/merge tools.
- **Repository manager** (open / clone / add, recent, tabs) and a fuzzy
  **command palette** (`Cmd/Ctrl + P`).
- **Hosting**: GitHub / GitLab / Bitbucket / Azure DevOps auth, PR/MR creation,
  remote-repo creation, GitHub notifications. Plus Git LFS, git-flow, submodules.
- **AI (bring-your-own-key)**: commit messages, Commit Composer, explain,
  conflict help, PR text, changelog, stash notes — local **Ollama** or remote
  providers, explicit opt-in, best-effort redaction. See
  [docs/AI-PRIVACY.md](docs/AI-PRIVACY.md).
- **MCP server** (`lady-mcp`) exposing read-only repo context to external AI
  assistants. See [docs/MCP.md](docs/MCP.md).
- Signed **auto-update**, light/dark + custom-accent theming, full keyboard +
  screen-reader support.

## Getting started

### Prerequisites

- **Rust** (stable — pinned by [`rust-toolchain.toml`](rust-toolchain.toml)).
  Install via [rustup](https://rustup.rs).
- **Node.js** 20+ and **npm** (builds the SolidJS UI).
- **System `git`** on your `PATH` — Lady shells out to it for credential-backed
  network ops and signing ([ADR-0003](docs/adr/0003-require-system-git-hybrid-engine.md)).
- **Tauri CLI**: `cargo install tauri-cli` (or use the bundled one,
  `npm --prefix ui exec tauri`).
- **Linux only** — WebKitGTK + GTK dev libs:
  ```sh
  sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
    libappindicator3-dev librsvg2-dev patchelf
  ```

### Clone & install

```sh
git clone https://github.com/ethanturk/lady.git
cd lady
npm --prefix ui install        # UI dependencies
```

### Run the app (dev)

```sh
cargo tauri dev
```

This launches the desktop app with hot-reload — it starts the Vite dev server
(`ui`, port 1420) and the Rust backend together. First launch begins a 30-day
trial; enter a license key any time under **Settings → License**
([ADR-0007](docs/adr/0007-licensing-gate-offline-signed-key.md)).

### Build an installer

```sh
cargo tauri build             # native package for the current OS
```

Produces a `.dmg`/`.app` (macOS), `.msi`/NSIS (Windows), or AppImage (Linux)
under `target/release/bundle/`. Signing/notarization and the cross-platform
release pipeline are described in [docs/PACKAGING.md](docs/PACKAGING.md).

### Mobile (iOS / Android)

Lady builds as an iOS and Android app with an adaptive UI (stacked on phones,
side-by-side on tablets/foldables). Full Git functionality is desktop-only —
phones get a degraded-but-usable layout. See [docs/MOBILE.md](docs/MOBILE.md)
for prerequisites and the `cargo tauri ios/android init|dev|build` commands.

### Tests & benchmarks

```sh
cargo test                                              # whole workspace
cargo bench -p lady-graph -p lady-diff -p lady-git      # perf (see docs/PERF.md)
npm --prefix ui run build                               # typecheck + build UI
```

## Documentation

| Doc | What |
| --- | --- |
| [docs/USER-GUIDE.md](docs/USER-GUIDE.md) | Open/clone, core flows, command palette |
| [docs/KEYBOARD.md](docs/KEYBOARD.md) | Keyboard shortcuts |
| [docs/AI-PRIVACY.md](docs/AI-PRIVACY.md) | BYOK, consent, redaction, local Ollama |
| [docs/MCP.md](docs/MCP.md) | `lady-mcp` setup for external assistants |
| [docs/MOBILE.md](docs/MOBILE.md) | iOS/Android build, adaptive UI, limitations |
| [docs/ACCESSIBILITY.md](docs/ACCESSIBILITY.md) | Keyboard / screen-reader / contrast |
| [docs/THEMING.md](docs/THEMING.md) | Theme token system |
| [docs/PACKAGING.md](docs/PACKAGING.md) | Bundling, signing, auto-update |
| [docs/PERF.md](docs/PERF.md) | Performance budgets + benchmarks |
| [docs/adr/](docs/adr/) | Architecture decision records |

## Architecture

Rust workspace ([Cargo.toml](Cargo.toml)): the Tauri app (`src-tauri`) over
focused crates — `lady-git` (engine over `gix` + system git), `lady-graph`
(lane layout), `lady-diff`, `lady-proto` (shared types), `lady-hosting`,
`lady-license`, `lady-ai`, `lady-mcp`, and the dev-only `lady-fixtures`. UI is
SolidJS in `ui/`. See [PLAN.md](PLAN.md) and the ADRs for the design.

## License

Proprietary (`LicenseRef-Proprietary`) — commercial, closed-source
([ADR-0004](docs/adr/0004-closed-source-commercial.md)). Dependencies are gated
to permissive licenses only via [`cargo deny`](deny.toml).
