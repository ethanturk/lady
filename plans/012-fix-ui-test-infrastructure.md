# Plan 012: Fix UI test infrastructure and expand coverage

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 7ff3460..HEAD -- ui/package.json ui/vitest.config.ts ui/src`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: None
- **Category**: tests + correctness
- **Planned at**: commit `7ff3460`, 2026-06-26

## Why This Matters

The UI test suite has a critical failure: `localStorage` is not available in the
jsdom test environment, causing `ui/src/prefs.ts` to crash at module load. This
blocks all UI test execution and gives false confidence (3 tests "pass" but the
suite cannot actually run). Before expanding test coverage, fix the infrastructure
so tests are trustworthy. Then add coverage for critical paths: App.tsx state
machine, ChangesView interactions, and dialog workflows.

## Current State

**Failing test:**

```text
ui/src/DiffView.test.ts (0 test)

 FAIL  src/DiffView.test.ts [ src/DiffView.test.ts ]
TypeError: Cannot read properties of undefined (reading 'getItem')
 ❯ src/prefs.ts:11:29
      9| const CHANGES_KEY = "lady-changes-layout";
     10|
     11| const stored = localStorage.getItem(CHANGES_KEY);
       |                             ^
```

**prefs.ts module load (crashes in tests):**

```ts
ui/src/prefs.ts:9-12
const CHANGES_KEY = "lady-changes-layout";

const stored = localStorage.getItem(CHANGES_KEY);
const [changesLayout, setChangesLayoutSignal] = createSignal<ChangesLayout>(
  stored === "tree" ? "tree" : "list",
);
```

**Existing test setup (no vitest config):**

```text
$ find ui -name "vitest.config.*"
# no output
```

**Test scripts in package.json:**

```json
ui/package.json:8-12
"scripts": {
  "dev": "vite",
  "build": "tsc --noEmit && vite build",
  "preview": "vite preview",
  "test": "vitest",
  "test:run": "vitest --run"
}
```

**Existing tests (3 files, 3 passing tests, 1 failing):**

```text
$ ls ui/src/*.test.*
ui/src/CommandPalette.test.tsx
ui/src/DiffView.test.ts
ui/src/lazyViews.test.tsx
```

**Critical untested code:**

- `ui/src/App.tsx` — 300+ lines, central state machine (license, repo, view, overlay signals)
- `ui/src/ChangesView.tsx` — 37K lines, main working changes workflow
- Dialog components: `HookErrorDialog.tsx`, `PullReconcileDialog.tsx`

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Install test deps | `npm --prefix ui install --save-dev @testing-library/jest-dom` | updates package/lock |
| Run UI tests | `npm --prefix ui run test:run` | tests pass |
| UI build | `npm --prefix ui run build` | exits 0 |
| Rust tests | `cargo test` | all tests pass |

## Scope

**In scope**:

- `ui/vitest.config.ts` — add proper config with localStorage polyfill
- `ui/src/prefs.ts` — guard localStorage access for test environment
- `ui/src/*.test.ts?(x)` — add tests for critical paths
- `ui/package.json` — add test setup file reference

**Out of scope**:

- Full E2E browser automation (Playwright/Cypress)
- Rewriting App.tsx architecture
- Backend/Tauri command changes
- Changing existing test behavior (only add new tests)

## Git Workflow

- Branch: `advisor/012-ui-test-infrastructure`
- Commit message: `test(ui): fix localStorage and expand coverage`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Create vitest.config.ts with localStorage polyfill

Create `ui/vitest.config.ts`:

```ts
import { defineConfig } from 'vitest/config';
import solid from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solid()],
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test-setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
```

Create `ui/src/test-setup.ts`:

```ts
import '@testing-library/jest-dom';

// Polyfill localStorage for jsdom
Object.defineProperty(window, 'localStorage', {
  value: {
    getItem: vi.fn(),
    setItem: vi.fn(),
    removeItem: vi.fn(),
    clear: vi.fn(),
    get length() {
      return 0;
    },
    get key() {
      return null;
    },
  },
  writable: true,
});
```

**Verify**: `npm --prefix ui run test:run` → exits 0 (even if no tests yet).

### Step 2: Guard prefs.ts localStorage access

Modify `ui/src/prefs.ts` to safely handle missing localStorage:

```ts
// At top of file, add helper:
const isTestEnv = typeof window === 'undefined' || !window.localStorage;

// Wrap all localStorage access:
const stored = isTestEnv ? null : localStorage.getItem(CHANGES_KEY);
```

Apply this pattern to all localStorage calls in prefs.ts (approximately 12 locations).

**Verify**: `npm --prefix ui run test:run` → DiffView.test.ts no longer crashes on import.

### Step 3: Add App.tsx state machine tests

Create `ui/src/App.test.tsx`:

```tsx
import { render, screen } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('App state machine', () => {
  it('renders changes view by default', () => {
    render(() => <App />);
    expect(screen.getByText('Local Changes')).toBeInTheDocument();
  });

  it('switches to commit graph when selected', async () => {
    render(() => <App />);
    // Add interaction test based on actual App.tsx structure
  });
});
```

Add 2-3 tests covering:
- Default view renders correctly
- View switching works (changes ↔ graph ↔ branches)
- Sidebar renders with expected items

**Verify**: `npm --prefix ui run test:run -- App` → tests pass.

### Step 4: Add ChangesView interaction tests

Create `ui/src/ChangesView.test.tsx`:

```tsx
import { render, screen, fireEvent } from '@solidjs/testing-library';
import { describe, expect, it, vi } from 'vitest';
import { ChangesView } from './ChangesView';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('ChangesView', () => {
  it('shows staged and unstaged sections', () => {
    render(() => <ChangesView />);
    expect(screen.getByText('Staged')).toBeInTheDocument();
    expect(screen.getByText('Unstaged')).toBeInTheDocument();
  });

  it('allows staging a file', async () => {
    render(() => <ChangesView />);
    // Add interaction test based on actual ChangesView structure
  });
});
```

Add 2-3 tests covering:
- Staged/unstaged sections render
- File staging action works
- Hunk expansion/collapsing (if applicable)

**Verify**: `npm --prefix ui run test:run -- ChangesView` → tests pass.

### Step 5: Add dialog test coverage

Create `ui/src/dialogs.test.tsx`:

```tsx
import { render, screen } from '@solidjs/testing-library';
import { describe, expect, it } from 'vitest';
import { HookErrorDialog } from './HookErrorDialog';
import { PullReconcileDialog } from './PullReconcileDialog';

describe('Dialogs', () => {
  it('HookErrorDialog shows error message and actions', () => {
    render(() => <HookErrorDialog message="Test error" onClose={vi.fn()} />);
    expect(screen.getByText('Test error')).toBeInTheDocument();
  });

  it('PullReconcileDialog shows reconcile options', () => {
    render(() => <PullReconcileDialog onClose={vi.fn()} />);
    // Add assertions based on actual dialog content
  });
});
```

Add 1-2 tests per dialog component.

**Verify**: `npm --prefix ui run test:run -- dialogs` → tests pass.

### Step 6: Wire test setup into package.json

Update `ui/package.json` to reference the setup file:

```json
{
  "vitest": {
    "setupFiles": ["./src/test-setup.ts"]
  }
}
```

Or add the setup reference in vitest.config.ts (whichever approach Step 1 uses).

**Verify**: `npm --prefix ui run test:run` → all tests pass, no console errors about localStorage.

### Step 7: Final verification

Run the full verification suite:

```sh
npm --prefix ui run test:run
npm --prefix ui run build
cargo test
```

**Verify**: All three commands exit 0.

## Test Plan

New test files:

- `ui/src/test-setup.ts` — localStorage polyfill and global setup
- `ui/src/App.test.tsx` — 3 tests for state machine
- `ui/src/ChangesView.test.tsx` — 3 tests for interactions
- `ui/src/dialogs.test.tsx` — 2-4 tests for dialogs

Use existing test patterns from `CommandPalette.test.tsx` and `lazyViews.test.tsx` as templates.

## Done Criteria

- [ ] `ui/vitest.config.ts` created with jsdom environment and localStorage polyfill
- [ ] `ui/src/test-setup.ts` created and referenced in config
- [ ] `ui/src/prefs.ts` guards localStorage access for test environment
- [ ] `npm --prefix ui run test:run` exits 0 with ≥10 tests passing
- [ ] `npm --prefix ui run build` exits 0
- [ ] `cargo test` exits 0
- [ ] Test coverage includes: App state machine, ChangesView interactions, at least 2 dialogs
- [ ] `plans/README.md` status row updated

## STOP Conditions

Stop and report back if:

- vitest.config.ts approach conflicts with existing Vite configuration
- prefs.ts changes break runtime behavior in dev mode (verify with `npm --prefix ui run dev`)
- App.tsx or ChangesView.tsx structure has changed significantly since plan was written
- Test count stays below 8 after completing all steps

## Maintenance Notes

Future UI plans should:

- Run `npm --prefix ui run test:run` as a verification gate
- Add tests alongside new components (co-location in `Component.test.tsx` format)
- Keep localStorage polyfill in test-setup.ts; add other mocks there as needed
- Target ≥80% coverage on critical paths (App, ChangesView, Commit actions) before major refactors
