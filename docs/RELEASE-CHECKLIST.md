# Lady — Core Parity + Fast-follow Release Checklist

This maps every item of the Core Parity surface (CONTEXT.md) to its implementing
story across Phases 1–3 (the v1.0 line, Phase 3 EXIT, PLAN.md §0/§9), plus the
Fast-follow set shipped in Phase 4 (v1.1.0), the AI set (Phase 5, v1.2.0), and
the **GA ship** (Phase 6, v1.3.0) — packaging, notarization, auto-update,
accessibility, theming, perf, docs, and the release pipeline.

Versions: Core Parity gate **1.0.0-rc**; Fast-follow gate **1.1.0**; AI gate
**1.2.0**; **GA ship 1.3.0 (current)**.

## Core Parity surface → implementing story

| Core Parity item | Status | Implementing story |
| --- | --- | --- |
| Commit graph (canvas, virtualized) | ✅ | PH1-004, PH1-005, PH1-006 |
| Diff viewer (+ syntax highlighting, split/unified) | ✅ | PH1-007, PH1-008 |
| Working & staged diffs | ✅ | PH2-003 |
| Partial staging — hunk level | ✅ | PH2-004 |
| Partial staging — line level + discard | ✅ | PH2-005 |
| Stage / unstage whole files | ✅ | PH2-002 |
| Working-tree status (Changes view) | ✅ | PH2-001 |
| Commit + amend | ✅ | PH2-006 |
| Branches + tags (create / delete / checkout) | ✅ | PH2-007 |
| Fetch / pull / push (system-git credentials + progress) | ✅ | PH2-008 |
| Merge (fast-forward / no-ff) + abort | ✅ | PH2-010 |
| Cherry-pick + revert | ✅ | PH2-011 |
| Stash (save / pop / apply / drop) | ✅ | PH2-009 |
| Drag-&-drop merge / rebase | ✅ | PH2-012 |
| 3-pane merge conflict resolver | ✅ | PH3-001 (engine), PH3-002 (UI) |
| Interactive rebase | ✅ | PH3-003 (engine), PH3-004 (UI) |
| GPG + SSH commit signing (+ verification badges) | ✅ | PH3-005 |
| Worktrees | ✅ | PH3-006 |
| Reflog (+ restore lost commits) | ✅ | PH3-007 |
| Bisect | ✅ | PH3-008 |
| Custom commands (typed-placeholder builder) | ✅ | PH3-009 |
| External diff / merge tool integration | ✅ | PH3-010 |
| Blame | ✅ | PH1-009 |
| File history | ✅ | PH1-010 |
| Repository manager (open / clone / add, recent, tabs) | ✅ | PH1-011 |
| Command palette | ✅ | PH1-012 |

## Release-blockers (Core Parity scope)

| Item | Status | Implementing story |
| --- | --- | --- |
| GitHub auth (OS-keychain token, ADR-0006) | ✅ | PH3-011 |
| Create GitHub pull request from a branch | ✅ | PH3-012 |
| Licensing gate — 30-day trial + offline signed-key verify (ADR-0007) | ✅ | PH3-013 |

## Fast-follow — shipped in Phase 4 (v1.1.0)

Committed post-v1.0 patches (CONTEXT.md), now implemented and green:

| Fast-follow item | Status | Implementing story |
| --- | --- | --- |
| Forge-agnostic hosting (HostingProvider trait) | ✅ | PH4-001 |
| GitLab auth + create merge request | ✅ | PH4-002 |
| Bitbucket auth + create pull request | ✅ | PH4-003 |
| Azure DevOps auth + create pull request | ✅ | PH4-004 |
| Create remote repository (all four forges) | ✅ | PH4-005 |
| GitHub notifications inbox | ✅ | PH4-006 |
| Git LFS support | ✅ | PH4-007 |
| git-flow support | ✅ | PH4-008 |
| Submodule management (incl. nested) | ✅ | PH4-009 |

## AI — GitKraken Git AI parity + superset, shipped in Phase 5 (v1.2.0)

BYOK + local Ollama, no hosted backend (ADR-0008); explicit opt-in, per-repo
toggle, best-effort redaction before any remote send (ADR-0009); thin
`reqwest` provider trait (ADR-0011). All providers tested vs wiremock, never
live.

| AI feature | Status | Implementing story |
| --- | --- | --- |
| Provider abstraction + task model + registry/config | ✅ | PH5-001 |
| BYOK key management + first-use consent + per-repo toggle | ✅ | PH5-002 |
| Local Ollama provider (never-leaves-your-machine path) | ✅ | PH5-003 |
| Remote providers (OpenAI, Anthropic, Gemini, Azure, Mistral) | ✅ | PH5-004 |
| Context builder + token budgeting + secret redaction | ✅ | PH5-005 |
| AI commit messages | ✅ | PH5-006 |
| Commit Composer (split into logical commits) | ✅ | PH5-007 |
| Explain (commit / branch range / stash / working changes) | ✅ | PH5-008 |
| AI conflict resolution (review-gated, never auto-written) | ✅ | PH5-009 |
| PR/MR title + description, changelog, stash notes | ✅ | PH5-010 |
| MCP server (lady-mcp) exposing repo context | ✅ | PH5-011 |
| Semantic commit search (stretch) | ⏸ deferred | PH5-012 |

Privacy posture verified: AI is off per repo until enabled; remote providers
are blocked until per-provider consent is recorded; the local Ollama path runs
with no consent gate and no mandatory redaction; redaction (regex + entropy) is
applied before every remote send and documented as best-effort.

PH5-012 (semantic commit search) is **explicitly deferred** — it is the marked
optional stretch and does not block the Phase 5 exit (PLAN.md §9; PRD PH5-013).
A literal `search_commits` (message grep) ships via the MCP server and the
engine; embedding-based semantic ranking is a Fast-follow/Phase 6 candidate.

## GA ship — Phase 6 (v1.3.0)

The cross-platform distribution + quality pass that turns the feature-complete
app into a signed, auto-updating, accessible, perf-budgeted product.

| GA item | Status | Implementing story | Reference |
| --- | --- | --- | --- |
| Performance benchmarks + budgets | ✅ | PH6-001 | [PERF.md](PERF.md) |
| Large-repo performance passes | ✅ | PH6-002 | [PERF.md](PERF.md) |
| Accessibility (keyboard, ARIA, AA contrast, reduced-motion) | ✅ | PH6-003 | [ACCESSIBILITY.md](ACCESSIBILITY.md), [KEYBOARD.md](KEYBOARD.md) |
| Theming (token system, System/Dark/Light, custom accent) | ✅ | PH6-004 | [THEMING.md](THEMING.md) |
| Packaging — macOS (.app/.dmg, notarized) | ✅ | PH6-005 | [PACKAGING.md](PACKAGING.md) |
| Packaging — Windows (.msi/NSIS, Authenticode) | ✅ | PH6-006 | [PACKAGING.md](PACKAGING.md) |
| Packaging — Linux (AppImage + Flatpak) | ✅ | PH6-007 | [PACKAGING.md](PACKAGING.md), [../flatpak](../flatpak) |
| Auto-update (Tauri updater + signed manifests) | ✅ | PH6-008 | [PACKAGING.md](PACKAGING.md), `src-tauri/src/updater.rs` |
| Documentation + release notes | ✅ | PH6-009 | [USER-GUIDE.md](USER-GUIDE.md), [AI-PRIVACY.md](AI-PRIVACY.md), [MCP.md](MCP.md), [../CHANGELOG.md](../CHANGELOG.md) |
| Release CI pipeline | ✅ | PH6-010 | `.github/workflows/release.yml` |

**GA ship line:** v1.3.0 — Core Parity + Fast-follow + AI, now signed,
auto-updating, accessible (WCAG AA), and perf-budgeted across macOS / Windows /
Linux. All signing/notarization/updater secrets are CI-only; only the updater
public key is committed.

### Post-GA (explicitly out of scope for the GA ship)

- Semantic commit search (PH5-012, marked optional stretch).
- Roving-tabindex arrow-key navigation within the view tablist.
- Automated axe-core a11y sweep in CI.
- High-fidelity production app-icon artwork (current icon is a valid placeholder).
- Telemetry / crash reporting (opt-in, per PLAN.md §8) — not built.

## Green-build gate (run before tagging)

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (whole workspace)
- [x] `cargo deny check`
- [x] `npm run build` (UI)
- [x] benches compile + run (`cargo bench --no-run` / `-- --quick`)
- [x] release workflow produces signed artifacts on the matrix (PH6-010)
