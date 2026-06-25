# Plan 003: Reconcile release docs with the shipped version

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- README.md CHANGELOG.md docs/RELEASE-CHECKLIST.md src-tauri/Cargo.toml src-tauri/tauri.conf.json ui/package.json crates`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: docs
- **Planned at**: commit `9666d42`, 2026-06-24

## Why This Matters

The top-level docs claim Lady is GA `v1.3.0`, but the current tag and packaged
app versions are `0.0.10`. That drift makes the README, changelog, release
checklist, and installer metadata disagree about what has actually shipped. The
fix is not to bump the product to GA; it is to make docs truthfully distinguish
current shipped version from phase roadmap/history.

## Current State

Relevant files:

- `README.md` - public-facing status line.
- `CHANGELOG.md` - release history and links.
- `docs/RELEASE-CHECKLIST.md` - readiness checklist and release claims.
- Version sources: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`,
  `ui/package.json`, `crates/*/Cargo.toml`.

Excerpts:

```text
README.md:7-9
**Status:** GA - **v1.3.0**. Core Parity + Fast-follow + AI, now signed,
auto-updating, accessible (WCAG AA), and performance-budgeted across macOS,
Windows, and Linux.
```

```text
src-tauri/tauri.conf.json:3-5
"productName": "Lady",
"version": "0.0.10",
"identifier": "dev.lady.client",
```

```json
ui/package.json:2-5
"name": "lady-ui",
"private": true,
"version": "0.0.10",
"type": "module",
```

```text
CHANGELOG.md:6-14
## [0.0.10] - Settings organization
...
## [1.3.0] - GA ship (Phase 6 - Polish & ship)
```

```text
docs/RELEASE-CHECKLIST.md:9-10
Versions: Core Parity gate 1.0.0-rc; Fast-follow gate 1.1.0; AI gate
1.2.0; GA ship 1.3.0 (current).
```

Observed git state during planning:

```text
git describe --tags --always --dirty
v0.0.10-dirty
```

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Version check | `git describe --tags --always --dirty` | starts with `v0.0.10` unless a newer tag exists |
| Search stale GA claims | `rg -n "GA|v1\\.3\\.0|1\\.3\\.0 \\(current\\)" README.md CHANGELOG.md docs` | no false current-version claims remain |
| UI build | `npm --prefix ui run build` | exits 0 |
| Rust tests | `cargo test` | all tests pass |

## Scope

**In scope**:

- `README.md`
- `CHANGELOG.md`
- `docs/RELEASE-CHECKLIST.md`
- Optional: small docs-only note in another `docs/*.md` file if needed to
  distinguish roadmap vs shipped version.

**Out of scope**:

- Changing any package/app version.
- Changing release workflow behavior.
- Removing historical phase notes that are still useful as roadmap context.
- Claiming features are absent without verifying product scope separately.

## Git Workflow

- Branch: `advisor/003-release-docs`
- Commit message: `docs: align release status with v0.0.10`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Define the wording policy

Use these rules:

- Current shipped status should say `0.0.10` if live versions still match the
  excerpts above.
- `1.0.0`, `1.1.0`, `1.2.0`, and `1.3.0` may remain only as phase roadmap,
  checklist targets, or historical intended milestones, not as current shipped
  release facts.
- Do not bump version numbers as part of this plan.

**Verify**: `git describe --tags --always --dirty` -> confirm current tag line
before editing.

### Step 2: Fix README status and release framing

Update `README.md` so the status line names the actual current release line.
Suggested shape:

```text
**Status:** active pre-GA build - current app/UI release **v0.0.10**.
The docs and checklist track the Core Parity/Fast-follow/AI/GA roadmap.
```

Keep the feature list unless you separately verify a listed feature is unbuilt;
the scope here is version truth, not product recertification.

**Verify**: `rg -n "Status:|v1\\.3\\.0|GA" README.md` -> README no longer
claims `v1.3.0` is current GA.

### Step 3: Move changelog GA sections under roadmap or mark them unreleased

Keep `## [0.0.10]` as the top actual release. Convert `1.0.0` through `1.3.0`
sections into either:

- `## [Unreleased roadmap: 1.3.0] ...`, or
- a dedicated `## Roadmap history` section with those phase notes.

Do not keep release links that point to tags which do not exist unless they are
clearly labeled future placeholders.

**Verify**: `rg -n "^## \\[1\\.|releases/tag/v1\\." CHANGELOG.md` -> any
matches are clearly marked unreleased/roadmap, not actual releases.

### Step 4: Fix release checklist "current" wording

Update `docs/RELEASE-CHECKLIST.md` so the Phase 6 section is a checklist target
or roadmap record, not "current" GA. Preserve useful checklist tables.

**Verify**: `rg -n "current\\)|GA ship line|v1\\.3\\.0" docs/RELEASE-CHECKLIST.md`
-> no line says `1.3.0` is current unless the actual versions were also changed
outside this plan, which is out of scope.

### Step 5: Run verification

Run the build/test smoke checks.

**Verify**: `npm --prefix ui run build` -> exits 0.

**Verify**: `cargo test` -> all tests pass.

## Test Plan

- Docs-only changes do not require new tests.
- Use `rg` commands above as the regression check for stale current-version
  claims.
- Run existing UI/Rust gates as smoke checks.

## Done Criteria

- [ ] README current status matches app/UI version.
- [ ] Changelog distinguishes actual `0.0.10` release from future/roadmap phase
  notes.
- [ ] Release checklist no longer calls `1.3.0` current unless versions were
  intentionally changed by a separate release plan.
- [ ] `npm --prefix ui run build` exits 0.
- [ ] `cargo test` exits 0.
- [ ] No package/app version files are changed.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report back if:

- The live `git describe` no longer starts with `v0.0.10` because a real GA tag
  landed after this plan was written.
- The operator wants to prepare a new release instead of fixing docs.
- You find signed artifacts or release workflow evidence proving `v1.3.0` has
  actually shipped.

## Maintenance Notes

Release docs should be updated in the same PR that changes `src-tauri` and UI
versions. Reviewers should compare `README.md`, `CHANGELOG.md`,
`docs/RELEASE-CHECKLIST.md`, `src-tauri/tauri.conf.json`, and `ui/package.json`
before tagging.
