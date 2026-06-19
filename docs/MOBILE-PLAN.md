# Plan: iOS/Android mobile app + responsive UI for Lady

## Context

Lady is a Tauri 2 desktop Git client (Rust core in `src-tauri/` + crates, SolidJS
WebView UI in `ui/`). Today it ships only for macOS/Windows/Linux. The goal is to
make it **buildable as an iOS and Android app** and to make the UI **adapt to
screen size**: full side-by-side panes on tablets/foldables (wide), and a
**stacked, top-to-bottom flow** on phones (narrow). Full mobile functionality on
small phone screens is explicitly NOT required — phones get a degraded-but-usable
layout; tablets/foldables get the full experience.

Two things make this more than a CSS pass:
- The app is already partly mobile-ready: it has **iOS + Android icons**
  (`src-tauri/icons/ios`, `src-tauri/icons/android`) and the lib crate is
  `crate-type = ["cdylib", "rlib"]`. But `run()` lacks the mobile entry point,
  and it uses the **desktop-only `tauri-plugin-updater`** unconditionally.
- The Rust backend shells out via `std::process::Command` for clone/fetch/pull/
  push, `open_url`, `open_path`, custom commands, etc. These compile on mobile but
  fail at runtime (no system `git`, no spawning on iOS). That is acceptable under
  the "not fully functional on phones" caveat and is documented, not solved here.

Reference repo `github.com/ethanturk/pex` has no mobile support itself; its only
relevance is its responsive styling approach (a wide→stacked layout). The
strategy below follows that spirit.

**Environment note:** this remote env has no Xcode/Android SDK, so the native
projects (`src-tauri/gen/apple`, `src-tauri/gen/android`) can't be generated or
built here. This plan wires everything up and documents the exact `tauri ios/
android init` + build commands; you run those locally (per your instruction).

---

## Part A — Responsive / adaptive UI (the verifiable core)

Strategy split: **structural layout switches** (row↔column, drawer, hide resize
handles, collapse toolbar) are driven by a reactive `viewportWidth` signal in
`prefs.ts` (Solid can't reactively flip `flex-direction` from a media query as
cleanly, and the codebase already gates JSX with `<Show>`). **Spacing / tap-target
/ safe-area** tweaks go in `styles.css` via media queries keyed on a new
`data-viewport` attribute on `<html>` and `@media (pointer: coarse)`.

Breakpoints: phone `< 768px` (narrow/stacked); tablet+foldable `>= 768px`
(side-by-side); optional `>= 1100px` "wide" reserved for future tuning.

### `ui/src/prefs.ts` — viewport tracking (reuse the existing density pattern)
Add module-scope signals mirroring the existing `data-text`/`data-pad` system:
- `viewportWidth` signal seeded from `window.innerWidth`, updated by a
  `requestAnimationFrame`-coalesced `resize` listener.
- Derived helpers exported: `isNarrow()` (`< 768`), `isWide()` (`>= 1100`),
  `coarsePointer()` (from `matchMedia("(pointer: coarse)")`), and
  `hideResizers()` = `isNarrow() || coarsePointer()`.
- `applyViewport(w)` sets `data-viewport="narrow|medium|wide"` on `<html>`.
- On first run with no stored `lady-ui-padding` AND coarse pointer, default
  `uiPadding` to `"l"` so existing `ps(...)` paddings grow tap targets — reuses
  the existing user-overridable density system instead of hard CSS overrides.

### `ui/src/App.tsx` — off-canvas drawer sidebar (hamburger)
- Import `isNarrow`, `hideResizers`. Add `const [drawerOpen, setDrawerOpen] =
  createSignal(false)`. Auto-close on navigation (`goPrimary`) and via
  `createEffect(() => { if (!isNarrow()) setDrawerOpen(false) })`.
- Body block (~L526–579): set `flex-direction: isNarrow() ? "column" : "row"`.
  Render `<Sidebar>` **inline only when `!isNarrow()`**; wrap the 6px col-resize
  handle in `<Show when={!hideResizers()}>`.
- Add a sibling, `position: fixed` **drawer + backdrop** shown only when
  `isNarrow() && drawerOpen()`: backdrop `rgba(0,0,0,0.45)` (z 900, click closes),
  panel `width: min(84vw,320px)` (z 901, `var(--panel)` bg, `drawerIn` slide
  animation, `padding-left: env(safe-area-inset-left)`), containing `<Sidebar>`.
- Give `Sidebar` a `fullWidth?: boolean` prop so it fills the drawer
  (`width: fullWidth ? "100%" : ${sidebarWidth()}px`).
- Safe-area: toast (`bottom: calc(20px + env(safe-area-inset-bottom,0px))`).

### `ui/src/Toolbar.tsx` — hamburger + collapse sync actions
- Add `onToggleSidebar?: () => void` prop; App passes `() => setDrawerOpen(v=>!v)`.
- Left group: show a hamburger `QuickAction` (new `IconMenu` in `icons.tsx`) first
  when `isNarrow()`; keep Launch always; wrap Fetch/Pull/Push/Stash in
  `<Show when={!isNarrow()}>`.
- On narrow, prepend Fetch/Pull/Push/Stash as items in the existing "More" menu
  (calling this component's local `fetch/pull/push/stash` directly, then closing
  the menu). The existing `overflowItems` loop is unchanged.
- Center repo•branch pill: `min-width: isNarrow() ? 0 : 230px` so it can shrink.

### `ui/src/ChangesView.tsx` — stack file-list / diff / composer
- Inner row container: `flex-direction: isNarrow() ? "column" : "row"`.
- File-lists column on narrow: `width:100%`, `max-height:45%`, `flex-shrink:1`,
  swap right-border→bottom-border. Diff wrapper: add `min-height:0`.
- Wrap the file-list col-resize handle AND the staged-pane row-resize handle in
  `<Show when={!hideResizers()}>`; on narrow the staged pane uses flexible height
  (`auto`) instead of the dragged `stagedHeight()`.
- Composer bottom inset: `padding-bottom: calc(<pad> + env(safe-area-inset-bottom,0px))`.

### `ui/src/ConflictResolver.tsx` — stack the 3 panes
- 3-pane container: `flex-direction: isNarrow() ? "column" : "row"`,
  `height: isNarrow() ? "auto" : "32%"`, add `max-height:55%`/`overflow:auto` on
  narrow. Panes: `flex:0 0 auto` + `min-height:5rem` and vertical→horizontal
  border swap so stacked panes get bottom borders. Hide the 10px minimap and let
  the header action bar `flex-wrap` on narrow.

### `ui/src/styles.css` + `ui/index.html`
- `index.html`: viewport meta → add `viewport-fit=cover` (required for `env()`).
- `styles.css`: add `@keyframes drawerIn` (auto-respects existing
  `prefers-reduced-motion`); `@media (pointer: coarse)` rules for a thicker
  scrollbar and larger menu/nav tap targets (scope min-height to menu/nav controls,
  not the tiny inline Stage/Unstage buttons); safe-area padding on the toolbar
  (`height: calc(58px + env(safe-area-inset-top,0px))`, `padding-top` inset).

### Known UI gaps to flag (not blockers)
- `AllCommitsView` is already a column (graph | 46% detail) — no row→column change;
  optionally bump detail to `60%` on narrow.
- `GraphView` multi-select needs Cmd/Shift-click — no touch equivalent; tap-to-
  select single still works. Call out as a future touch affordance.

---

## Part B — Mobile platform wiring (Rust / Tauri)

### `src-tauri/src/lib.rs`
- Annotate the entry point: `#[cfg_attr(mobile, tauri::mobile_entry_point)] pub fn run()`.
- Gate the desktop-only updater plugin behind `#[cfg(desktop)]` (builder rebind):
  ```rust
  let builder = tauri::Builder::default();
  #[cfg(desktop)]
  let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
  builder.plugin(tauri_plugin_dialog::init()) /* …rest unchanged… */
  ```
- Make `mod updater;` and the two `updater::*` handler entries `#[cfg(desktop)]`,
  splitting `generate_handler!` so mobile omits them (desktop keeps them).

### `src-tauri/Cargo.toml`
- Move `tauri-plugin-updater` to `[target.'cfg(desktop)'.dependencies]` so it
  doesn't compile on mobile targets.

### `src-tauri/capabilities/`
- Keep `default.json` (core+dialog, `windows:["main"]`) for all platforms; add a
  `desktop.json` capability holding `updater:default` with
  `"platforms": ["macOS","windows","linux"]` so mobile never references the
  missing updater permission.

### `src-tauri/tauri.conf.json`
- Add `bundle.android` (`minSdkVersion: 24`) and `bundle.iOS`
  (`minimumSystemVersion: "13.0"`, developmentTeam via `TAURI_APPLE_DEVELOPMENT_TEAM`
  env). Existing `identifier` `dev.lady.client` is valid for both stores.

### `ui/vite.config.ts`
- Add mobile dev-server support: read `TAURI_DEV_HOST`, set `server.host` and
  `server.hmr` accordingly (physical-device HMR), keep `port: 1420` / `strictPort`.

### `docs/MOBILE.md` (new) + `README.md` link
- Prerequisites: Xcode + iOS targets (`aarch64-apple-ios`, `…-sim`,
  `x86_64-apple-ios`); Android Studio/SDK/NDK + targets (`aarch64-linux-android`
  et al.); `cargo install tauri-cli`.
- Commands: `cargo tauri android init`, `cargo tauri ios init`, then
  `cargo tauri android dev` / `cargo tauri ios dev`, and `… build`.
- Document mobile limitations: process-based git ops (clone/fetch/pull/push,
  custom commands, open/reveal) and auto-update are desktop-only and error on
  mobile; the keyring-backed hosting token store may need a mobile fallback
  (noted as a follow-up risk — `keyring` Android support should be verified at
  `init`/build time).
- `.gitignore`: ignore generated mobile build artifacts (the `tauri init`
  commands also write their own `gen/*/.gitignore`).

---

## Critical files
- UI: `ui/src/prefs.ts`, `ui/src/App.tsx`, `ui/src/Toolbar.tsx`,
  `ui/src/ChangesView.tsx`, `ui/src/ConflictResolver.tsx`, `ui/src/icons.tsx`,
  `ui/src/styles.css`, `ui/index.html`.
- Backend: `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json` (+ new
  `desktop.json`), `ui/vite.config.ts`, new `docs/MOBILE.md`.

## Verification
1. `npm --prefix ui run build` — typecheck (`tsc --noEmit`) + Vite build passes.
2. `cargo check` (desktop) passes — confirms the `#[cfg(desktop)]` gating and
   split handler compile; `cargo test -p lady-app` still green.
3. Manual responsive check via `cargo tauri dev`: shrink the window below 768px →
   sidebar collapses to a hamburger drawer, Changes/Conflict panes stack
   vertically, sync actions move into the More menu; widen ≥768px → full
   side-by-side returns. Test at text sizes S and XL (zoom × safe-area).
4. Mobile (run locally, has SDKs): `cargo tauri android init && cargo tauri
   android dev`; `cargo tauri ios init && cargo tauri ios dev` — app launches and
   the stacked layout renders on a phone profile / side-by-side on a tablet
   profile.

## Commit
Commit the changes with a descriptive message and push to `main` (per your
instruction). The session branch `claude/mobile-app-ios-android-h0r7qm` is also
available if you'd prefer a PR instead.
