# Plan 006: Add a Tauri CSP for the packaged app

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the next
> step. If anything in the "STOP conditions" section occurs, stop and report -
> do not improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9666d42..HEAD -- src-tauri/tauri.conf.json ui/src/DiffView.tsx ui/src ui/package.json`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/004-add-ui-test-coverage.md
- **Category**: security
- **Planned at**: commit `9666d42`, 2026-06-24

## Why This Matters

The packaged app currently disables CSP with `"csp": null`. The UI mostly
renders text safely, but the diff viewer intentionally uses `innerHTML` for
highlight.js output from repository-controlled file contents. A CSP is a
defense-in-depth layer: it reduces blast radius if a future HTML sink, image
data URL, or dependency bug bypasses escaping.

## Current State

Relevant files:

- `src-tauri/tauri.conf.json` - packaged app security config.
- `ui/src/DiffView.tsx` - highlighted diff lines and image data URLs.
- `ui/src/styles.css` and UI components - many inline styles currently exist.

Excerpts:

```json
src-tauri/tauri.conf.json:23-25
"security": {
  "csp": null
}
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

```tsx
ui/src/DiffView.tsx:351
<span style={{ ...codeStyle(wrapDiff()), color: codeColorFor(kind) }} innerHTML={highlight(nl.line.content, props.lang)} />
```

```tsx
ui/src/DiffView.tsx:431
<img src={`data:${imageMime(props.file.path)};base64,${props.file.old_image_b64}`} ... />
```

Important constraint: the UI uses many Solid `style={{ ... }}` attributes, so a
strict CSP that forbids inline styles would require a large styling refactor.
Start with a realistic CSP that blocks script/object/network surprises while
allowing current inline styles.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| UI tests | `npm --prefix ui run test:run` | exits 0 |
| UI build | `npm --prefix ui run build` | exits 0 |
| Tauri build smoke | `cargo tauri build --debug` | exits 0 and produces a debug bundle |
| Rust tests | `cargo test` | all tests pass |

## Scope

**In scope**:

- `src-tauri/tauri.conf.json`
- UI tests from plan 004 if needed for CSP-sensitive rendering.
- Minimal UI code changes only if a CSP-compatible rendering path is required.

**Out of scope**:

- Rewriting all inline styles into CSS classes.
- Removing highlight.js.
- Changing AI provider/network behavior.
- Changing updater endpoint behavior beyond allowing the existing signed update
  endpoint if needed.

## Git Workflow

- Branch: `advisor/006-tauri-csp`
- Commit message: `security: add packaged app csp`
- Do not push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add a realistic CSP string

Replace `"csp": null` with a CSP that starts from least privilege but allows
current app behavior. Suggested starting point:

```json
"csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: https:; connect-src 'self' http://localhost:* http://127.0.0.1:* https://github.com https://api.github.com https://gitlab.com https://api.bitbucket.org https://dev.azure.com https://*.openai.com https://api.anthropic.com https://generativelanguage.googleapis.com https://api.mistral.ai; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"
```

Adjust only as needed for actual app requirements. Keep `object-src 'none'` and
`frame-ancestors 'none'`.

**Verify**: `npm --prefix ui run build` -> exits 0.

### Step 2: Add/extend tests for HTML sink behavior

If plan 004 added `DiffView` tests, extend them to assert repository-controlled
HTML-like content is escaped or rendered as text. If plan 004 used a different
file, add a focused `DiffView` test now.

**Verify**: `npm --prefix ui run test:run -- DiffView` -> tests pass.

### Step 3: Build a Tauri debug bundle

Run a debug bundle build so the config is parsed by Tauri. This writes under
ignored build outputs.

**Verify**: `cargo tauri build --debug` -> exits 0. If system packaging
dependencies are missing on the local OS, run `cargo build --all-targets`
instead and note the skipped Tauri bundle in the plan status.

### Step 4: Run full smoke gates

**Verify**: `npm --prefix ui run test:run` -> exits 0.

**Verify**: `npm --prefix ui run build` -> exits 0.

**Verify**: `cargo test` -> all tests pass.

## Test Plan

- Diff rendering test for HTML-like file contents.
- Existing UI tests from plan 004.
- Tauri config/build smoke via `cargo tauri build --debug` where available.

## Done Criteria

- [ ] `src-tauri/tauri.conf.json` no longer has `"csp": null`.
- [ ] CSP includes `object-src 'none'` and `frame-ancestors 'none'`.
- [ ] CSP allows current required app behavior without broad `default-src *`.
- [ ] `npm --prefix ui run test:run` exits 0.
- [ ] `npm --prefix ui run build` exits 0.
- [ ] `cargo test` exits 0.
- [ ] Tauri config parsed by `cargo tauri build --debug` or an explicit note
  explains why local packaging smoke was skipped.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report back if:

- Plan 004 has not landed and there is no UI test command.
- A working CSP requires allowing `script-src 'unsafe-inline'`.
- The app requires arbitrary remote origins in `connect-src`.
- The only way to pass CSP is a broad styling rewrite.

## Maintenance Notes

Whenever a new provider, updater endpoint, or embedded asset scheme is added,
review CSP at the same time. Avoid weakening `script-src`, `object-src`, or
`frame-ancestors` for convenience.

