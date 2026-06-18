# Lady — Core Parity + Fast-follow Release Checklist

This maps every item of the Core Parity surface (CONTEXT.md) to its implementing
story across Phases 1–3 (the v1.0 line, Phase 3 EXIT, PLAN.md §0/§9), plus the
Fast-follow set shipped in Phase 4 (v1.1.0). It is **not** the ship itself —
packaging / notarization / auto-update are Phase 6.

Versions: Core Parity gate **1.0.0-rc**; Fast-follow gate **1.1.0** (current).

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

## Not yet (later phases)

- **AI features** — Phase 5 (PLAN.md; ADR-0008/0009/0011).
- **Packaging / notarization / auto-update** — Phase 6. These gates are
  feature-completeness, not the actual ship.

## Green-build gate (run before tagging)

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo test` (whole workspace)
- [x] `cargo deny check`
- [x] `npm run build` (UI)
