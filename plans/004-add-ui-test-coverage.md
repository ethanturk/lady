# Plan 004: Add automated UI coverage

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- ui/package.json ui/package-lock.json ui/src ui/vite.config.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: plans/001-add-agent-executor-guide.md recommended
- **Category**: tests
- **Planned at**: commit `9666d42`, 2026-06-24

## Why This Matters

The Rust layer has broad unit/integration coverage, but the Solid UI package has
no test script and no component/browser tests. Most product workflow state lives
in `ui/src/App.tsx` and feature components: repository switching, staged vs
unstaged views, worktrees, dialogs, settings, and AI flows. Before doing bundle
splitting or CSP work, add a small UI test baseline so regressions are caught by
automation instead of manual app use.

## Current State

Relevant files:

- `ui/package.json` - has `dev`, `build`, `preview`; no `test`.
- `ui/src/App.tsx` - central stateful app shell with many imported views.
- `ui/src/DiffView.tsx` - pure-ish rendering logic worth covering first.
- `ui/src/CommandPalette.tsx`, `ui/src/prefs.ts` - small UI/state utilities
  suitable for tests.

Excerpts:

```json
ui/package.json:6-9
"scripts": {
  "dev": "vite",
  "build": "tsc --noEmit && vite build",
  "preview": "vite preview"
}
```

```ts
ui/src/App.tsx:69-78
const App: Component = () => {
  const [license, setLicense] = createSignal<LicenseStatus | null>(null);
  const [unread, setUnread] = createSignal(0);
  const [active, setActive] = createSignal<OpenRepo | null>(null);
  const [refs, setRefs] = createSignal<RefInfo[]>([]);
  const [repositoryFamily, setRepositoryFamily] = createSignal<RepositoryFamily | null>(null);
  const [view, setView] = createSignal<PrimaryView>("changes");
  const [overlay, setOverlay] = createSignal<Overlay | null>(null);
```

```ts
ui/src/DiffView.tsx:73-83
const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function highlight(content: string, lang: string | undefined): string {
  if (!content) return "&nbsp;";
  if (!lang) return escapeHtml(content);
  try {
    return hljs.highlight(content, { language: lang, ignoreIllegals: true }).value;
```

There are currently no UI test files:

```text
find ui/src -type f \( -name '*.test.*' -o -name '*.spec.*' \)
# no output
```

Repo convention:

- Keep UI code in TypeScript/Solid.
- Use existing Vite build tooling rather than introducing a separate bundler.
- Preserve `npm --prefix ui run build` as the production build gate.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Add test deps | `npm --prefix ui install --save-dev vitest @solidjs/testing-library jsdom @testing-library/jest-dom` | updates package and lockfile |
| Run UI tests | `npm --prefix ui run test -- --run` | tests pass once added |
| UI build | `npm --prefix ui run build` | exits 0 |
| Rust tests | `cargo test` | all tests pass |

## Scope

**In scope**:

- `ui/package.json`
- `ui/package-lock.json`
- `ui/vitest.config.ts` or equivalent test config
- `ui/src/**/*.test.ts`
- `ui/src/**/*.test.tsx`
- Small refactors in UI source only when required to export pure helpers for
  testability.

**Out of scope**:

- Full end-to-end desktop automation.
- Rewriting `App.tsx`.
- Changing backend commands.
- Adding a browser automation stack unless component tests cannot cover the
  baseline cases.

## Git Workflow

- Branch: `advisor/004-ui-test-coverage`
- Commit message: `test(ui): add component test baseline`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add Vitest-based UI test tooling

Install Vitest and Solid Testing Library. Add scripts:

```json
"test": "vitest",
"test:run": "vitest --run"
```

Add a `ui/vitest.config.ts` that uses the Solid/Vite plugin, `environment:
"jsdom"`, and includes `src/**/*.test.ts?(x)` files.

**Verify**: `npm --prefix ui run test:run` -> exits 0 after config is present,
even if it initially reports no tests only before Step 2.

### Step 2: Add pure rendering/unit tests for diff safety

If needed, export small helpers from `ui/src/DiffView.tsx` such as
`escapeHtml`, `highlight`, `langFromPath`, or test behavior via rendered
components. Add tests covering:

- raw `<script>`-shaped content is escaped when no language is known;
- known languages still highlight without throwing;
- empty content renders a non-empty placeholder.

Keep exports narrow and do not change runtime behavior.

**Verify**: `npm --prefix ui run test:run -- DiffView` -> tests pass.

### Step 3: Add a small interactive component test

Choose one low-dependency component, preferably `CommandPalette` or a small
dialog/control, and test a real interaction such as filtering and selecting an
entry. Mock Tauri `invoke` only if needed.

**Verify**: `npm --prefix ui run test:run` -> all UI tests pass.

### Step 4: Wire the new test gate into local package scripts

Ensure `npm --prefix ui run test:run` is the stable non-watch command future
plans can use. Do not add it to CI in this plan unless the operator asks.

**Verify**: `npm --prefix ui run build` -> exits 0.

**Verify**: `cargo test` -> all tests pass.

## Test Plan

New tests:

- `ui/src/DiffView.test.ts` or `ui/src/DiffView.test.tsx` for escaping and
  highlighting behavior.
- One component interaction test for `CommandPalette` or another small,
  low-dependency component.

Use `ui/src/DiffView.tsx` helper structure as the pattern: keep pure helpers
near the component, export only what tests need.

## Done Criteria

- [ ] `ui/package.json` has `test` and `test:run` scripts.
- [ ] `npm --prefix ui run test:run` exits 0 and runs at least two meaningful UI
  tests.
- [ ] `npm --prefix ui run build` exits 0.
- [ ] `cargo test` exits 0.
- [ ] Test helpers do not weaken escaping behavior.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report back if:

- Vitest/Solid Testing Library cannot be installed without forcing a major
  framework migration.
- Tests require large rewrites of `App.tsx`.
- The current code at `DiffView.tsx:73-83` no longer matches the escaping
  excerpt.
- `npm --prefix ui run build` fails twice after reasonable test-config fixes.

## Maintenance Notes

Future UI plans should run `npm --prefix ui run test:run` in addition to the
existing build. If the suite becomes slow, split fast component tests from any
future browser/e2e tests rather than removing the gate.

