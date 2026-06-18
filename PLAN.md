# Lady — A Fast, Friendly Git Client in Rust

> Goal: duplicate the functionality of [Fork](https://fork.dev/) (a fast & friendly native Git GUI for macOS/Windows) and add AI features at parity with [GitKraken Git AI](https://www.gitkraken.com/features/git-ai), implemented in **Rust**.

This document is the master plan: feature inventory mined from both sites, technology decisions, architecture, a crate-by-crate module breakdown, the genuinely hard problems and how to solve them, the AI subsystem design, and a phased roadmap.

---

## 0. Locked Decisions (grilled)

These were resolved in a grilling session and are authoritative — where anything below conflicts, these win. Terms in **bold-italic** are defined in [CONTEXT.md](CONTEXT.md); each links its ADR in [docs/adr/](docs/adr/).

| # | Decision | Record |
|---|----------|--------|
| North star | ***Ship fast*** = build velocity to ***Core Parity***, **not** early public release. | [CONTEXT](CONTEXT.md) |
| GUI | **Tauri v2** (Rust core + web frontend); domain logic stays GUI-agnostic so a native shell can swap in later. | [ADR-0001](docs/adr/0001-gui-framework-tauri.md) |
| Release | **Big-bang at parity** — first public release gates on ***Core Parity*** (Phases 1–3). No thin MVP. AI ships after. | [ADR-0002](docs/adr/0002-release-at-fork-parity.md) |
| Parity bar | ***Core Parity*** = Fork's daily-use surface; niche long-tail (LFS, git-flow, submodule edges, non-GitHub forge PRs) is ***Fast-follow***. | [CONTEXT](CONTEXT.md) |
| Git engine | **Require system git** (v1); hybrid `gix` (reads) + `git2` (gaps) + shell-out git (mutations/niche). Authenticated network ops → shell-out tier. | [ADR-0003](docs/adr/0003-require-system-git-hybrid-engine.md) |
| Platforms | **macOS-primary** dev/dogfood; Windows + Linux **build+test in CI from Phase 0** (no port debt). | — |
| Business | **Closed-source commercial** (Fork model: trial + paid). Hard rule: permissive deps only, **no GPL/AGPL** — enforce via `cargo-deny`. libgit2's linking exception is OK. | [ADR-0004](docs/adr/0004-closed-source-commercial.md) |
| Commit graph | **Canvas 2D** for lanes/edges + virtualized **DOM** for row text/refs (hybrid); render-target swappable. | [ADR-0005](docs/adr/0005-commit-graph-canvas.md) |
| Auth/signing | **Reuse system git's** credential helpers + ssh-agent + gpg/ssh signing config. `keyring` holds only hosting-API tokens. | [ADR-0006](docs/adr/0006-auth-and-signing-via-system-git.md) |
| Licensing gate | **Offline Ed25519-signed key + 30-day trial**, client-side; in ***Core Parity*** scope. No server v1. Not DRM — never gate security-sensitive paths behind it. | [ADR-0007](docs/adr/0007-licensing-gate-offline-signed-key.md) |
| AI hosting | **BYOK + local Ollama**, no Lady inference backend. Paid tier unlocks AI *features*; user covers inference. | [ADR-0008](docs/adr/0008-ai-byok-local-ollama.md) |
| AI privacy | **Explicit opt-in, no silent cloud** — first-use consent, secret-redaction before remote send, per-repo AI toggle, Ollama as the private path. | [ADR-0009](docs/adr/0009-ai-privacy-explicit-opt-in.md) |

**Scope deltas vs the original roadmap below:**
- **Release line** sits at the end of **Phase 3** (Core Parity). Phase 4 (hosting) is GitHub-only at v1; other forges are ***Fast-follow***. Phase 5 (AI) is strictly post-release.
- A **licensing-gate module** (trial + signed-key verify) is added to Core Parity — it is release-blocking, not Fast-follow.
- **`cargo-deny`** license + advisory checks run in CI from **Phase 0**.
- **Open (grill at their phase):** AI provider abstraction (`reqwest` trait vs `genai`/`rig`), MCP tool scope, Tauri frontend framework (Solid/Svelte), commit-graph lane algorithm.

---

## 1. Feature Inventory (mined)

### 1.1 Fork feature parity targets

Mined from [fork.dev](https://fork.dev/), [git-fork.com](https://git-fork.com/), the [release notes](https://fork.dev/releasenotes), [Quick Launch post](https://fork.dev/blog/posts/quick-launch/), and third-party reviews.

| Area | Feature | Notes |
|------|---------|-------|
| **Core git ops** | Fetch, pull, push | Background, with progress |
| | Commit, amend | With recent-message access |
| | Create/delete branches & tags | |
| | Create/delete remote repos | Via hosting APIs |
| | Checkout branch or revision | Detached HEAD aware |
| | Cherry-pick, revert | |
| | Merge | Incl. fast-forward/no-ff options |
| | Rebase (plain + interactive) | Visual edit/reorder/squash/drop/fixup |
| | Stash (save/pop/apply/drop) | Stashes shown inline in commit list |
| | Submodules | Init/update/sync |
| | Worktrees | Create & manage |
| | Reflog | "Restore lost commits" |
| | Bisect | Standard git capability |
| **Staging** | Stage/unstage line-by-line & hunk-by-hunk | Partial staging |
| | Discard hunks/lines | |
| **Commit graph** | Visual DAG with lanes, refs, avatars | The signature view |
| | Star indicator on tabs for uncommitted changes | |
| **Diff viewer** | Side-by-side & unified | |
| | Syntax highlighting (many languages) | |
| | Image diffs (common formats) | |
| | Conflict markers on scrollbars | Minimap-style |
| | External diff/merge tool support | Configurable |
| **Merge conflicts** | Built-in 3-pane merge resolver | Line-by-line, ours/theirs/result |
| **History/analysis** | Blame view (last commit per line) | |
| | File history (commits affecting a path) | |
| | Browse repo file tree at any commit | |
| **Repository manager** | Create / clone / add existing | |
| | Recent repositories | |
| | Tabs + custom groups | Reproduces source folder structure |
| **Productivity** | Command palette / Quick Launch (⌘/Ctrl+P) | Fuzzy action search |
| | Custom commands (UI builder: text fields, branch combos, file pickers) | |
| | Drag & drop merge/rebase (drag branch onto branch) | |
| **Security** | GPG **and** SSH commit signing | |
| | SSH key support / agent | |
| **Integrations** | Git LFS | |
| | Git-flow | |
| | Create PR on GitHub / GitLab / Bitbucket / Azure DevOps | Via branch context menu |
| | GitHub notifications | |
| | (Recent) Claude Code integration for branch reviews & commit messages | We exceed this with §5 |
| **UX** | Dark theme + theming | |
| | Native, fast, low-memory | Core selling point |
| **Platforms** | macOS 10.11+, Windows 7+ | We add Linux |

### 1.2 GitKraken Git AI parity targets

Mined from [gitkraken.com/features/git-ai](https://www.gitkraken.com/features/git-ai).

| AI feature | Description |
|------------|-------------|
| **Commit Composer** | Analyze working changes, **organize them into logical commits**, and craft clear, review-ready messages. |
| **AI commit messages** | Generate a message for the current staged/working set. |
| **Auto-resolve merge conflicts with AI** | Suggest fixes, explain the conflict, resolve with confidence. |
| **Explain (plain-English)** | Instant explanations of commits, branches, stashes, or working changes; regenerate option. |
| **Auto-generate content** | PR titles, stash notes, changelogs. |
| **MCP server** | Expose full repo context to external assistants (Copilot, Cursor, Windsurf, Claude). |
| **Provider choice / BYOK** | OpenAI, Google Gemini, Anthropic Claude, Azure, Mistral, Ollama, or **bring your own API key**. |
| **Surfaces** | Desktop, IDE plugin, CLI, MCP. |

**Our AI superset** (beyond GitKraken, leveraging the desktop context): semantic commit search ("find where we changed retry logic"), PR description from a branch's full diff, interactive-rebase plan suggestions, blame-aware "explain this line's history," and a local-first Ollama path for privacy.

---

## 2. Technology Decisions

These are the load-bearing choices. Each is a recommendation with rationale, not a survey.

### 2.1 Git engine — **hybrid: `gix` + `git2` + shell-out**

No single Rust git library covers everything a Fork-class client needs. Use three tiers:

1. **`gix` (gitoxide)** — primary for read & performance-critical paths: object/ref access, **commit graph traversal**, status, diff, blame, log/file-history, packed-refs, commit-graph file. Pure Rust, fastest, async-friendly building blocks, actively developed.
2. **`git2` (libgit2)** — for operations `gix` doesn't yet expose cleanly: some merge/rebase, cherry-pick, stash apply, index manipulation edge cases.
3. **System `git` (shell-out via `tokio::process`)** — for the long tail that must match git exactly: interactive rebase *todo* execution, hooks, clean/smudge filters & **LFS**, credential helpers, GPG/SSH **signing**, `git-flow`, bisect driving. Parse porcelain v2 / `-z` machine output.

> Rationale: this mirrors how production GUIs hedge. We get gitoxide's speed where it matters and git's correctness/feature-completeness where it's risky to reimplement. Abstract all three behind one `GitEngine` trait so call sites don't care which tier serves a request.

### 2.2 GUI framework — **Tauri v2 (primary recommendation)**, with a pure-Rust fallback noted

The UI is dense, virtualized, and visually rich (commit graph, diff panes, trees). Two viable directions:

- **Recommended: Tauri v2.** Rust core + web frontend (TypeScript + a fast framework like Solid/Svelte). Pros: fastest path to a polished, themeable, accessible UI; trivial virtualization & syntax highlighting in the web layer; small bundle vs Electron (system WebView); first-class cross-platform incl. Linux; Rust owns all git/AI logic and exposes it via `#[tauri::command]` + events. Cons: a JS layer to maintain; canvas needed for the highest-perf graph.
- **Pure-Rust alternative: `iced` or `egui` (eframe) or `Slint`.** Pros: single language, GPU-rendered, no WebView, closest to Fork's "native & tiny" ethos. Cons: you build virtualization, rich text, and accessibility yourself; longer road to Fork-level polish.

> Decision: **start on Tauri v2** to reach feature parity fastest and keep 100% of domain logic in Rust crates that are GUI-agnostic. Keep the core crates UI-independent so a future `iced`/`Slint` native shell can be swapped in without touching git/AI logic. The commit graph renders to `<canvas>` (or WebGL) for performance.

### 2.3 Supporting stack

| Concern | Choice | Why |
|---------|--------|-----|
| Async runtime | `tokio` | Background git/network, `spawn_blocking` for sync `gix`/`git2` |
| Diff algorithm | `imara-diff` | Myers + Histogram, fast, used by gitoxide |
| 3-way merge | `imara-diff` + diff3 logic / `git2` merge | For built-in resolver |
| Syntax highlighting | `tree-sitter` (per-language grammars) | Incremental, accurate; fallback `syntect` for breadth |
| HTTP | `reqwest` (rustls) | AI providers + hosting REST APIs |
| Serialization | `serde` / `serde_json` | Settings, API payloads, AI tool schemas |
| Secure secrets | `keyring` | OS keychain for tokens & AI API keys (never plaintext) |
| FS watching | `notify` | Detect external repo changes → refresh |
| Fuzzy search | `nucleo` | Command palette, branch/file pickers |
| Logging/trace | `tracing` + `tracing-subscriber` | Structured diagnostics |
| Token counting | `tiktoken-rs` | AI context budgeting |
| MCP | `rmcp` (official Rust MCP SDK) | Expose repo context to external assistants |
| Error handling | `thiserror` (libs) + `anyhow` (app) | |
| License/advisory gate | `cargo-deny` in CI (Phase 0) | Enforce permissive-only deps ([ADR-0004](docs/adr/0004-closed-source-commercial.md)) |
| Config | `serde` + TOML in platform config dir (`directories`) | |
| Auto-update | Tauri updater plugin | Signed releases |

---

## 3. Architecture

Layered, with a hard boundary between **GUI** and **domain core** so the UI can be swapped.

```
┌──────────────────────────────────────────────────────────────┐
│  UI Shell (Tauri webview: Solid/Svelte + canvas graph)       │
│  views: graph · diff · staging · merge · blame · history ·    │
│         repo-manager · command-palette · settings · AI panel │
└───────────────▲───────────────────────────┬──────────────────┘
        events  │            commands        │ (#[tauri::command])
┌───────────────┴───────────────────────────▼──────────────────┐
│  app-core (Rust): AppState, repo sessions, command bus,       │
│  background job queue, undo/redo, watch→refresh, settings      │
└──┬───────────┬───────────┬───────────┬───────────┬────────────┘
   │           │           │           │           │
┌──▼───┐  ┌────▼────┐  ┌───▼────┐  ┌───▼────┐  ┌───▼─────┐
│ git- │  │  diff   │  │  ai    │  │ hosting│  │  mcp    │
│engine│  │ /merge  │  │ service│  │ (forge)│  │ server  │
│(gix/ │  │ /blame  │  │(providers│ │GH/GL/  │  │(rmcp)   │
│git2/ │  │ engine) │  │ +context)│ │BB/ADO) │  │         │
│ sh)  │  └─────────┘  └────────┘  └────────┘  └─────────┘
└──────┘
```

### 3.1 Threading & responsiveness

- UI thread never blocks. Every git/network/AI call is a **job** dispatched to a background executor.
- Sync libraries (`gix`, `git2`) run under `tokio::task::spawn_blocking` or a dedicated `rayon` pool for CPU-heavy work (diffing thousands of files, graph layout).
- Jobs report progress via an event channel → UI shows determinate/indeterminate progress, and are **cancellable** (cooperative cancellation tokens).
- A single-writer model per repo: mutating ops (commit, rebase, merge) are serialized through a per-repo actor to avoid index races; reads run concurrently.

### 3.2 State & refresh

- `notify` watches `.git/` and worktree; debounced (e.g. 150 ms) refresh of status/refs.
- Diff against an in-memory snapshot; only re-render changed regions (the UI keeps stable IDs per commit/file for virtualization).

---

## 4. Workspace & module breakdown

Cargo workspace; domain crates are GUI-free and independently testable.

```
lady/
├── Cargo.toml                      # workspace
├── crates/
│   ├── lady-git/                   # GitEngine trait + gix/git2/shell impls
│   │   ├── refs, objects, status
│   │   ├── log (graph walk, file history)
│   │   ├── ops (commit, branch, merge, rebase, cherry-pick, revert,
│   │   │        stash, tag, worktree, submodule, bisect, reflog)
│   │   ├── interactive_rebase (todo model + executor)
│   │   ├── signing (gpg / ssh-keygen -Y)
│   │   ├── lfs (filter passthrough)
│   │   └── credentials (helper protocol + keyring)
│   ├── lady-diff/                  # imara-diff wrappers, hunk/line model,
│   │   │                           # partial-stage patch builder, image diff
│   │   └── merge/                  # 3-way merge, conflict model, resolver ops
│   ├── lady-syntax/                # tree-sitter highlighting → styled spans
│   ├── lady-graph/                 # commit-graph LANE LAYOUT algorithm
│   ├── lady-ai/                    # provider trait + impls, context builder,
│   │   │                           # token budgeting, commit-splitter, prompts
│   │   └── providers/ (openai, anthropic, gemini, azure, mistral, ollama)
│   ├── lady-hosting/               # GitHub/GitLab/Bitbucket/Azure DevOps:
│   │   │                           # auth, PR create, notifications, repo create
│   ├── lady-mcp/                   # MCP server exposing repo-context tools
│   ├── lady-core/                  # AppState, jobs, settings, watch, undo
│   └── lady-proto/                 # shared serde types (UI <-> core contract)
├── ui/                             # Tauri frontend (Solid/Svelte + canvas)
└── src-tauri/                      # Tauri shell, command bindings, events
```

### 4.1 Key types (sketch)

```rust
// lady-git
pub trait GitEngine: Send + Sync {
    fn status(&self, repo: &RepoId) -> Result<WorkingTree>;
    fn graph(&self, repo: &RepoId, q: GraphQuery) -> Result<CommitPage>;
    fn diff(&self, repo: &RepoId, spec: DiffSpec) -> Result<FileDiff>;
    fn stage(&self, repo: &RepoId, patch: PartialPatch) -> Result<()>; // line/hunk
    fn commit(&self, repo: &RepoId, msg: &str, opts: CommitOpts) -> Result<Oid>;
    fn rebase_interactive(&self, repo: &RepoId, plan: RebasePlan) -> Result<JobId>;
    fn blame(&self, repo: &RepoId, path: &Path, at: Oid) -> Result<Blame>;
    // ... merge, cherry_pick, stash, worktree, bisect, reflog, signing ...
}

// lady-graph — lane assignment for the DAG
pub struct GraphRow { pub oid: Oid, pub lane: u16, pub edges: Vec<Edge>, pub refs: Vec<RefDeco> }

// lady-ai
#[async_trait] pub trait AiProvider: Send + Sync {
    async fn complete(&self, req: AiRequest) -> Result<AiResponse>;
    fn id(&self) -> ProviderId; fn context_window(&self) -> usize;
}
pub enum AiTask { CommitMessage, SplitCommits, Explain(Target), ResolveConflict,
                  PrTitle, PrDescription, Changelog, StashNote }
```

---

## 5. AI subsystem design (`lady-ai`)

Parity with GitKraken Git AI plus a privacy-first, local-capable design.

### 5.1 Provider abstraction & BYOK

- One `AiProvider` trait; impls for **OpenAI, Anthropic, Gemini, Azure OpenAI, Mistral, Ollama**. (Consider `genai`/`rig-core` to avoid hand-rolling six clients, but a thin `reqwest` trait keeps control of streaming & token limits.)
- **BYOK:** keys stored in OS keychain (`keyring`), never on disk in plaintext, never logged. Per-feature model selection (e.g. cheap model for messages, strong model for conflict resolution).
- **Local-first option:** Ollama path so a privacy-sensitive user never sends code off-machine. Surfaced prominently.
- **Streaming:** stream tokens to the UI for responsive generation; cancellable.

### 5.2 Context builder (the real work)

AI quality depends on what we feed it. Pipeline:

1. **Gather**: relevant diff (staged/working/commit range), file paths, surrounding hunk context, branch name, recent commit messages (style priming), repo conventions (Conventional Commits if detected).
2. **Budget**: count tokens (`tiktoken-rs`); rank hunks by salience; **chunk & summarize** large diffs (map-reduce: summarize per-file, then synthesize). Hard cap on bytes sent.
3. **Redact**: optional secret-scanning pass (entropy + regex) to strip obvious credentials before sending to a remote provider.
4. **Prompt**: task-specific templates with few-shot examples; structured output (JSON) for machine-consumed results (e.g. commit-split plan).

### 5.3 Feature implementations

- **Commit message** — diff → message; honor detected convention; editable preview.
- **Commit Composer (split into logical commits)** — feed full working diff; ask model to group hunks into N commits with messages; return a plan `[{message, [hunk_ids]}]`; we apply via partial staging (`lady-diff` patch builder) then commit each group. This reuses the line/hunk staging engine — a strong reason to build that first.
- **Explain** (commit / branch / stash / working changes) — read-only; "regenerate" re-rolls with higher temperature.
- **Auto-resolve conflict** — feed both sides + base + surrounding context per conflict region; model returns resolved hunk; user reviews in the 3-pane resolver before accepting (never auto-write without confirmation).
- **PR title/description, changelog, stash note** — summarize a commit range; changelog groups by Conventional-Commit type.
- **(Superset) semantic search** — embed commit messages + diffs (provider embeddings or local), vector store (e.g. `hnsw`/`qdrant`-lite or SQLite + brute force for v1) for "find where X changed."

### 5.4 MCP server (`lady-mcp`)

- Expose repo context as MCP **tools/resources** via `rmcp`: `get_status`, `get_diff`, `get_log`, `get_file_at`, `blame`, `search_commits`.
- Lets external assistants (Claude Desktop, Cursor, Windsurf, Copilot) drive Lady's repo knowledge — matching GitKraken's MCP story, and making Lady a context provider, not just a consumer.

### 5.5 Safety/UX rules

- AI is **opt-in**; explicit consent before first remote send; clear indicator of which provider/model and whether data leaves the machine.
- All AI-mutating actions (apply split, write resolution) require human confirmation.
- Graceful degradation: every AI feature has a non-AI path.

---

## 6. The hard problems (and the approach)

1. **Commit graph layout** (`lady-graph`) — assign each commit a horizontal *lane*, route parent/child edges, handle octopus merges, keep layout stable as you scroll/refresh. Stream rows from the walk; lay out incrementally; render visible rows to canvas. Reference prior art (`git-graph`) but implement our own stable, incremental layout. **This is the signature feature — budget real time.**
2. **Virtualization at scale** — repos with 1M+ commits, 50k-file trees. Everything paginated/windowed: graph rows, diffs, trees, blame. Never materialize whole history.
3. **Partial staging** (`lady-diff`) — build precise patches for arbitrary line/hunk subsets and apply to the index without corrupting it. Foundation for both manual staging *and* AI Commit Composer.
4. **Interactive rebase** — model the todo list (pick/reword/edit/squash/fixup/drop/reorder/exec) as data; execute reliably with stop-on-conflict, continue/abort, and recovery. Safest path: drive `git rebase` with a generated todo + sequence editor shim, surfacing state back to the UI.
5. **3-pane merge resolver** (`lady-diff::merge`) — base/ours/theirs/result with per-hunk take-ours/take-theirs/edit, conflict minimap on scrollbars; write resolved blob + mark resolved.
6. **Signing** — GPG and SSH (`ssh-keygen -Y sign`/git's `gpg.ssh`), agent integration, passphrase prompts via the credential UI.
7. **Credentials** — implement the git credential-helper protocol, store tokens in `keyring`, support OAuth device-flow for hosting providers.
8. **Cross-platform parity** — path/encoding/line-ending differences, file watching quirks, keychain backends, packaging & code-signing for macOS (notarization), Windows (Authenticode), Linux (AppImage/Flatpak).
9. **Performance budget** — cold open of a large repo < ~1s to first paint; status refresh < ~100ms on warm cache; smooth 60fps graph scroll. Measure continuously.

---

## 7. Hosting integrations (`lady-hosting`)

Provider trait with impls for **GitHub, GitLab, Bitbucket, Azure DevOps**:

- OAuth (device flow) / PAT auth → tokens in keychain.
- **Create PR/MR** from a branch (matches Fork's context-menu PR creation).
- **Create remote repo**.
- **Notifications** (GitHub first — Fork parity), surfaced in a tray/inbox view.
- PR status checks / review state (stretch).

---

## 8. Cross-platform, packaging, updates

- **Targets:** macOS (Apple Silicon + Intel), Windows x64, Linux (we extend beyond Fork).
- **Packaging:** Tauri bundler → `.dmg`/`.app` (notarized), `.msi`/NSIS (Authenticode-signed), AppImage + Flatpak.
- **Auto-update:** Tauri updater with signed manifests.
- **Telemetry:** off by default, opt-in, anonymized; crash reports via `sentry` (opt-in).

---

## 9. Phased roadmap

Effort tags are relative size, not calendar promises.

### Phase 0 — Foundations (M)
- Workspace, CI (fmt/clippy/test on 3 OSes), `GitEngine` trait + `gix`/`git2`/shell wiring, Tauri shell, settings, logging.
- **Exit:** open a repo, list refs, walk log to a flat list.

### Phase 1 — Read-only foundation (L)
- Commit **graph** (lane layout + canvas render), commit details, **diff viewer** (side-by-side, syntax highlighting, image diff), file tree at commit, **blame**, **file history**, repository manager (clone/add/recent, tabs+groups), command palette.
- **Exit:** browse any repo as well as Fork's read views.

### Phase 2 — Write operations (L)
- Staging (line/hunk partial), commit/amend, branches/tags, checkout, fetch/pull/push (with credentials), stash, merge, cherry-pick, revert, drag-&-drop merge/rebase.
- **Exit:** daily-driver for common workflows.

### Phase 3 — Advanced git + ship (L) → **🚢 RELEASE LINE (v1.0 = Core Parity)**
- **Interactive rebase**, **3-pane merge resolver**, worktrees, reflog, bisect, **signing (GPG+SSH via system git)**, custom commands, external diff/merge tools.
- **Licensing-gate module** (30-day trial + offline signed-key verify) — release-blocking ([ADR-0007](docs/adr/0007-licensing-gate-offline-signed-key.md)).
- GitHub-only PR creation folded in here (the one forge at v1).
- **Deferred to Fast-follow** (post-v1.0 patches): LFS, git-flow, submodule edge cases, PR creation for GitLab/Bitbucket/Azure DevOps.
- **Exit:** ***Core Parity*** — first public release.

### Phase 4 — Fast-follow hosting + niche (M) — *post-release*
- Remaining forges (GitLab/Bitbucket/Azure DevOps) auth + PR/MR create, remote-repo create, GitHub notifications; LFS, git-flow, submodule edges.

### Phase 5 — AI (L) — GitKraken parity + superset — *post-release*
- Provider abstraction + **BYOK + Ollama** ([ADR-0008](docs/adr/0008-ai-byok-local-ollama.md)) with **explicit opt-in + redaction** ([ADR-0009](docs/adr/0009-ai-privacy-explicit-opt-in.md)); commit messages; **Commit Composer** (logical split); explain commit/branch/stash/working; **AI conflict resolution** (review-gated); PR title/description; changelog; stash notes.
- **MCP server** for external assistants.
- Stretch: semantic commit search.
- **Exit:** GitKraken Git AI parity, local-first option, MCP context provider.

### Phase 6 — Polish & ship (M)
- Theming, accessibility, perf passes (large-repo benchmarks), packaging/signing/notarization, auto-update, docs.

---

## 10. Testing & quality

- **Unit:** diff/merge/graph-layout/partial-stage are pure → property tests (`proptest`) against git's own output as oracle.
- **Integration:** spin throwaway repos in fixtures; assert engine ops match `git` CLI results.
- **Snapshot:** graph layout & diff rendering via `insta`.
- **AI:** golden-prompt tests with recorded provider responses (`wiremock`); never hit live APIs in CI; redaction unit tests are mandatory.
- **Perf:** criterion benchmarks on large synthetic repos (linux.git-scale); track first-paint & refresh budgets.
- **Cross-platform CI:** macOS/Windows/Linux matrix.

---

## 11. Key risks

| Risk | Mitigation |
|------|-----------|
| Commit-graph layout complexity | Prototype `lady-graph` in Phase 1; reuse `git-graph` insights; incremental + stable layout |
| `gix` feature gaps | Hybrid engine — fall back to `git2`/shell; isolate behind trait |
| Interactive rebase correctness | Drive real `git rebase`; extensive integration tests; robust abort/recover |
| AI sends sensitive code | Opt-in, redaction pass, local Ollama path, explicit provider/destination indicator |
| Native polish vs Fork on Tauri | Canvas graph for perf; keep core GUI-agnostic so a native `iced`/`Slint` shell can replace the webview later |
| Cross-platform signing/notarization | Tackle packaging early (Phase 0 stub) so it isn't a launch blocker |

---

## 12. Immediate next steps

1. Scaffold the Cargo workspace + `lady-proto` contract + Tauri shell (Phase 0).
2. Spike `lady-git` over `gix`: open repo, walk log, emit `GraphRow`s.
3. Spike `lady-graph` lane layout against a real repo; render to canvas.
4. Decide provider strategy for `lady-ai` (thin `reqwest` trait vs `genai`/`rig`) and stand up the OpenAI + Ollama impls behind `AiProvider`.

---

### Sources
- [Fork — fork.dev](https://fork.dev/) · [git-fork.com](https://git-fork.com/) · [release notes](https://fork.dev/releasenotes) · [Quick Launch view](https://fork.dev/blog/posts/quick-launch/)
- [GitKraken Git AI](https://www.gitkraken.com/features/git-ai)
- [Best Git GUI Clients 2026 (review)](https://thesoftwarescout.com/best-git-clients-2026-top-gui-tools-for-version-control/) · [DEV: Git GUI comparison](https://dev.to/_d7eb1c1703182e3ce1782/best-git-gui-clients-in-2025-gitkraken-sourcetree-fork-and-more-compared-4gjd)
