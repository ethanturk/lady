# Plan 005: Split the frontend bundle by lazy-loading advanced views

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- ui/src/App.tsx ui/src ui/package.json ui/vite.config.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/004-add-ui-test-coverage.md
- **Category**: perf
- **Planned at**: commit `9666d42`, 2026-06-24

## Why This Matters

`npm --prefix ui run build` succeeds but emits one large application chunk:
`dist/assets/index-*.js` around 1.2 MB minified and a Vite warning above 500 kB.
The app shell eagerly imports many advanced views that are not needed for first
paint. Lazy-loading those views should reduce startup bytes while preserving the
core repository/changes/commit experience.

## Current State

Relevant files:

- `ui/src/App.tsx` - eagerly imports all primary and advanced views.
- `ui/src/index.tsx` - root render.
- `ui/vite.config.ts` - current build config, no manual chunking.
- `ui/package.json` - build script.

Excerpts:

```ts
ui/src/App.tsx:6-44
import AllCommitsView from "./AllCommitsView";
import BlameView from "./BlameView";
import FileHistory from "./FileHistory";
import ChangesView from "./ChangesView";
...
import AiView from "./AiView";
import LicenseGate from "./LicenseGate";
import CommandPalette from "./CommandPalette";
```

```ts
ui/src/App.tsx:47-63
type Overlay =
  | "refs"
  | "blame"
  | "history"
  | "conflicts"
  | "worktrees"
  | "reflog"
  | "bisect"
  | "commands"
  | "settings"
  | "notifications"
  | "lfs"
  | "flow"
  | "submodules"
  | "stashes"
  | "ai";
```

```ts
ui/vite.config.ts:27-31
build: {
  target: "chrome105",
  minify: "esbuild",
  sourcemap: false,
},
```

Observed build output before this plan:

```text
dist/assets/index-*.js 1,215.17 kB | gzip: 389.45 kB
(!) Some chunks are larger than 500 kB after minification.
```

Repo UI convention:

- SolidJS with colocated components in `ui/src`.
- Keep the first screen as the actual app, not a landing page.
- Preserve keyboard/accessibility behavior.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| UI tests | `npm --prefix ui run test:run` | exits 0 |
| UI build | `npm --prefix ui run build` | exits 0 |
| Bundle check | `ls -lh ui/dist/assets/*.js` | multiple JS chunks; initial chunk meaningfully smaller than baseline |
| Rust tests | `cargo test` | all tests pass |

## Scope

**In scope**:

- `ui/src/App.tsx`
- Optional small helper file such as `ui/src/lazyViews.tsx`
- `ui/vite.config.ts` only if manual chunking is needed after route-level lazy
  loading.
- UI tests added in plan 004 if they need updates for async/lazy rendering.

**Out of scope**:

- Changing backend commands.
- Replacing SolidJS or Vite.
- Visual redesign.
- Lazy-loading the core first-screen path if it makes startup feel blank.

## Git Workflow

- Branch: `advisor/005-split-frontend-bundle`
- Commit message: `perf(ui): lazy-load advanced views`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Identify the first-screen critical path

Keep these eager unless measurement proves otherwise:

- `App`
- `RepoBar`
- `Toolbar`
- `Sidebar`
- `ChangesView`
- `AllCommitsView` if it is part of the default repository workflow
- `CommandPalette` if keyboard launch needs instant availability
- `LicenseGate`

Lazy-load advanced overlays:

- `RefsView`, `BlameView`, `FileHistory`, `ConflictResolver`,
  `InteractiveRebase`, `RecomposeView`, `ExplainPanel`, `WorktreesView`,
  `ReflogView`, `BisectView`, `CustomCommandsView`, `SettingsView`,
  `NotificationsView`, `LfsView`, `GitFlowView`, `SubmodulesView`,
  `StashView`, `AiView`.

**Verify**: `npm --prefix ui run test:run` -> existing tests pass before edits.

### Step 2: Introduce Solid lazy boundaries

Use Solid's `lazy` and `Suspense` around advanced views. Keep prop types intact.
Use a small, unobtrusive loading fallback that fits the existing app surface,
for example a text/status line in the content area. Do not show a marketing
loader or change navigation structure.

**Verify**: `npm --prefix ui run test:run` -> tests pass; update tests only for
legitimate async rendering.

### Step 3: Build and inspect chunk output

Run the production build.

**Verify**: `npm --prefix ui run build` -> exits 0.

**Verify**: `ls -lh ui/dist/assets/*.js` -> more than one JS file exists and
the main `index-*.js` file is meaningfully smaller than the pre-plan 1.2 MB
baseline.

If Vite still emits one very large chunk, add minimal `manualChunks` in
`ui/vite.config.ts` for obvious groups such as `ai`, `settings`, and
`advanced-git`. Keep the config simple.

### Step 4: Smoke core navigation

Run UI tests and Rust tests.

**Verify**: `npm --prefix ui run test:run` -> exits 0.

**Verify**: `cargo test` -> all tests pass.

## Test Plan

- Update the plan-004 component tests if lazy boundaries require awaiting
  rendered content.
- Add one test that renders the shell with a lazy advanced view and confirms the
  fallback is replaced by the view content.
- Use existing tests from plan 004 as the structural pattern.

## Done Criteria

- [ ] Advanced views are lazy-loaded through Solid `lazy`/`Suspense` or an
  equivalent Vite-compatible dynamic import.
- [ ] Core first-screen path remains eager and usable.
- [ ] `npm --prefix ui run test:run` exits 0.
- [ ] `npm --prefix ui run build` exits 0.
- [ ] Build output has multiple JS chunks and a smaller initial chunk than the
  1.2 MB baseline.
- [ ] `cargo test` exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report back if:

- Plan 004 has not landed and there is no UI test command.
- Lazy-loading requires broad prop/state rewrites in `App.tsx`.
- Build output does not improve after both route-level lazy loading and one
  simple manual chunk attempt.
- A lazy fallback breaks keyboard navigation or traps focus.

## Maintenance Notes

When adding new rarely used views, import them lazily by default. Keep core
startup views eager so the desktop app does not flash an empty shell.

