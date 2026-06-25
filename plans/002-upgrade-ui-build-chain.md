# Plan 002: Upgrade the UI build chain and clear npm audit

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- ui/package.json ui/package-lock.json ui/vite.config.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `9666d42`, 2026-06-24

## Why This Matters

The Rust security/license gate passes, but the UI dependency gate fails:
`npm --prefix ui audit --audit-level=high` reports the Vite -> esbuild dev
server advisory chain. Even if the vulnerable server is primarily a dev-time
surface, a commercial desktop app should not carry a known high audit failure
when the UI build is part of the release pipeline. Clearing this restores a
single green dependency baseline for both Rust and Node.

## Current State

Relevant files:

- `ui/package.json` - declares Vite and `vite-plugin-solid`.
- `ui/package-lock.json` - locks Vite and esbuild versions.
- `ui/vite.config.ts` - Tauri-compatible Vite config; preserve behavior.

Excerpts:

```json
ui/package.json:17-21
"devDependencies": {
  "@tauri-apps/cli": "^2",
  "typescript": "^5.4",
  "vite": "^5.3",
  "vite-plugin-solid": "^2.10"
}
```

```text
ui/package-lock.json:1536-1548
"node_modules/esbuild": {
  "version": "0.21.5",
  ...
  "engines": {
    "node": ">=12"
  }
}
```

```text
ui/package-lock.json:1940-1948
"node_modules/vite": {
  "version": "5.4.21",
  ...
  "dependencies": {
    "esbuild": "^0.21.3",
```

```ts
ui/vite.config.ts:8-31
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: {
    host: host || false,
    port: 1420,
    strictPort: true,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});
```

Observed failure before this plan:

```text
npm --prefix ui audit --audit-level=high
esbuild <=0.24.2
vite <=6.4.2 depends on vulnerable versions of esbuild
2 vulnerabilities (1 moderate, 1 high)
```

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Baseline audit | `npm --prefix ui audit --audit-level=high` | currently exits non-zero before the fix |
| Update deps | `npm --prefix ui install --save-dev vite@latest vite-plugin-solid@latest` | updates `ui/package.json` and lockfile |
| UI build | `npm --prefix ui run build` | `tsc --noEmit && vite build` exits 0 |
| Audit gate | `npm --prefix ui audit --audit-level=high` | exits 0 |
| Rust smoke | `cargo test` | all tests pass |

## Scope

**In scope**:

- `ui/package.json`
- `ui/package-lock.json`
- `ui/vite.config.ts` only if the Vite upgrade requires config syntax updates.

**Out of scope**:

- UI feature changes.
- Tauri Rust config changes.
- Replacing Vite, SolidJS, or Tauri.
- Applying `npm audit fix --force` blindly if it changes unrelated packages or
  rewrites the app architecture.

## Git Workflow

- Branch: `advisor/002-upgrade-ui-build-chain`
- Commit message: `build(ui): upgrade vite toolchain`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Confirm the current audit failure

Run the audit gate from repo root.

**Verify**: `npm --prefix ui audit --audit-level=high` -> fails with the Vite /
esbuild advisory chain. If it already passes, skip to Step 4 and only update
the plan status as DONE with a note that it was fixed independently.

### Step 2: Upgrade the Vite toolchain

Use npm to update Vite and the Solid Vite plugin together so peer dependencies
stay compatible:

```sh
npm --prefix ui install --save-dev vite@latest vite-plugin-solid@latest
```

Preserve the existing Tauri dev-server behavior in `ui/vite.config.ts`: port
`1420`, `strictPort: true`, mobile `TAURI_DEV_HOST` handling, HMR port `1421`,
`envPrefix`, and build target/minify settings unless Vite rejects them.

**Verify**: `git diff -- ui/package.json ui/package-lock.json ui/vite.config.ts`
-> changes are limited to dependency versions/lockfile and any required Vite
config compatibility edits.

### Step 3: Build the UI

Run the existing build script.

**Verify**: `npm --prefix ui run build` -> exits 0. A chunk-size warning may
remain; plan 005 addresses bundle splitting.

### Step 4: Re-run audit and Rust tests

Run the dependency audit and the Rust suite.

**Verify**: `npm --prefix ui audit --audit-level=high` -> exits 0.

**Verify**: `cargo test` -> all tests pass.

## Test Plan

- Existing UI build is the required type/build test.
- Existing Rust tests are a smoke check that the Tauri workspace still compiles
  against the updated UI toolchain.
- No new tests are required in this plan.

## Done Criteria

- [ ] `ui/package-lock.json` no longer locks `esbuild` to a vulnerable version.
- [ ] `npm --prefix ui audit --audit-level=high` exits 0.
- [ ] `npm --prefix ui run build` exits 0.
- [ ] `cargo test` exits 0.
- [ ] No files outside the in-scope list and `plans/README.md` are modified.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report back if:

- The upgrade requires changing Tauri major versions.
- `vite-plugin-solid` has no compatible release for the selected Vite version.
- `npm --prefix ui run build` fails twice after reasonable config fixes.
- Clearing audit would require replacing SolidJS or changing application code
  outside `ui/vite.config.ts`.

## Maintenance Notes

Add `npm --prefix ui audit --audit-level=high` to CI in a follow-up only if the
team wants Node advisories to be release-blocking. This plan clears the current
failure; it does not change CI policy.

