import { createSignal } from "solid-js";

/**
 * Small UI preferences persisted in localStorage (like the theme), shared
 * reactively across components via module-scope signals.
 */
export type ChangesLayout = "list" | "tree";

const CHANGES_KEY = "lady-changes-layout";

const stored = localStorage.getItem(CHANGES_KEY);
const [changesLayout, setChangesLayoutSignal] = createSignal<ChangesLayout>(
  stored === "tree" ? "tree" : "list",
);

export { changesLayout };

/** Set how the Local Changes file lists render: flat list or directory tree. */
export function setChangesLayout(v: ChangesLayout): void {
  setChangesLayoutSignal(v);
  localStorage.setItem(CHANGES_KEY, v);
}

// ── Diff line wrapping ───────────────────────────────────────────────────────
// Off by default: long lines stay on one line and the diff scrolls sideways.
// When on, lines wrap to the viewport so nothing is clipped horizontally.
const WRAP_KEY = "lady-diff-wrap";
const [wrapDiff, setWrapDiffSignal] = createSignal<boolean>(
  localStorage.getItem(WRAP_KEY) === "1",
);

export { wrapDiff };

/** Toggle forced line-wrapping in diffs (persisted). */
export function setWrapDiff(v: boolean): void {
  setWrapDiffSignal(v);
  localStorage.setItem(WRAP_KEY, v ? "1" : "0");
}

// ── Auto-update check on launch ──────────────────────────────────────────────
// On by default: at launch Lady checks the signed update endpoint and, if a
// newer version exists, shows a non-blocking banner. Installing stays an
// explicit user click — the check is the only thing automated here.
const AUTO_UPDATE_KEY = "lady-auto-update-check";
const [autoUpdateCheck, setAutoUpdateCheckSignal] = createSignal<boolean>(
  localStorage.getItem(AUTO_UPDATE_KEY) !== "0",
);

export { autoUpdateCheck };

/** Toggle the launch-time check for new releases (persisted). */
export function setAutoUpdateCheck(v: boolean): void {
  setAutoUpdateCheckSignal(v);
  localStorage.setItem(AUTO_UPDATE_KEY, v ? "1" : "0");
}

// ── Resizable Staged pane height (Local Changes) ─────────────────────────────
const STAGED_H_KEY = "lady-staged-height";
const storedH = Number(localStorage.getItem(STAGED_H_KEY));
const [stagedHeight, setStagedHeightSignal] = createSignal<number>(
  Number.isFinite(storedH) && storedH > 0 ? storedH : 260,
);

export { stagedHeight };

/** Persist the Staged pane height (px) for the Local Changes split. */
export function setStagedHeight(px: number): void {
  setStagedHeightSignal(px);
  localStorage.setItem(STAGED_H_KEY, String(Math.round(px)));
}

// ── Adjustable pane / dialog widths (px) ─────────────────────────────────────
function widthPref(key: string, fallback: number) {
  const stored = Number(localStorage.getItem(key));
  const [get, set] = createSignal<number>(Number.isFinite(stored) && stored > 0 ? stored : fallback);
  const setter = (px: number) => {
    set(px);
    localStorage.setItem(key, String(Math.round(px)));
  };
  return [get, setter] as const;
}

const [sidebarWidth, setSidebarWidth] = widthPref("lady-sidebar-width", 248);
const [changesColWidth, setChangesColWidth] = widthPref("lady-changes-col-width", 308);
const [settingsWidth, setSettingsWidth] = widthPref("lady-settings-width", 640);
const [commitDetailHeight, setCommitDetailHeight] = widthPref("lady-commit-detail-height", 340);
// Conflict resolver's Combined pane height. 0 ⇒ unset (default to 1/3 on mount).
const [conflictCombinedHeight, setConflictCombinedHeight] = widthPref("lady-conflict-combined-height", 0);

export {
  conflictCombinedHeight,
  setConflictCombinedHeight,
  sidebarWidth,
  setSidebarWidth,
  changesColWidth,
  setChangesColWidth,
  settingsWidth,
  setSettingsWidth,
  commitDetailHeight,
  setCommitDetailHeight,
};

// ── UI density: text size + padding (S / M / L / XL) ─────────────────────────
export type SizeStep = "s" | "m" | "l" | "xl";
const SIZE_STEPS: SizeStep[] = ["s", "m", "l", "xl"];
const asStep = (v: string | null): SizeStep => (SIZE_STEPS.includes(v as SizeStep) ? (v as SizeStep) : "m");

const TEXT_KEY = "lady-text-size";
const PAD_KEY = "lady-ui-padding";

const [textSize, setTextSizeSignal] = createSignal<SizeStep>(asStep(localStorage.getItem(TEXT_KEY)));
const [uiPadding, setUiPaddingSignal] = createSignal<SizeStep>(asStep(localStorage.getItem(PAD_KEY)));

export { textSize, uiPadding };

// Reflect the steps onto <html> so styles.css can drive the root zoom (text)
// and the --pad-scale variable (padding) globally. Applied at module load and
// on every change.
function applyText(v: SizeStep): void {
  document.documentElement.setAttribute("data-text", v);
}
function applyPad(v: SizeStep): void {
  document.documentElement.setAttribute("data-pad", v);
}
applyText(textSize());
applyPad(uiPadding());

/** Set the global text size (root zoom): smaller / default / larger / largest. */
export function setTextSize(v: SizeStep): void {
  setTextSizeSignal(v);
  localStorage.setItem(TEXT_KEY, v);
  applyText(v);
}

/** Set the global UI padding density via the --pad-scale variable. */
export function setUiPadding(v: SizeStep): void {
  setUiPaddingSignal(v);
  localStorage.setItem(PAD_KEY, v);
  applyPad(v);
}

/** Label for a size step (used by the settings selectors). */
export const SIZE_LABEL: Record<SizeStep, string> = { s: "S", m: "M", l: "L", xl: "XL" };
export const SIZE_OPTIONS = SIZE_STEPS;

// ── Color scheme + accent (moved out of the toolbar into Settings) ───────────
export type ThemeMode = "system" | "dark" | "light";
export type Accent = "default" | "teal";
export const THEME_MODES: ThemeMode[] = ["system", "dark", "light"];
export const ACCENTS: Accent[] = ["default", "teal"];

const THEME_KEY = "lady-theme";
const ACCENT_KEY = "lady-accent";

const prefersDark = () =>
  window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;

function applyTheme(mode: ThemeMode): void {
  const resolved = mode === "system" ? (prefersDark() ? "dark" : "light") : mode;
  document.documentElement.setAttribute("data-theme", resolved);
}
function applyAccent(a: Accent): void {
  if (a === "default") document.documentElement.removeAttribute("data-accent");
  else document.documentElement.setAttribute("data-accent", a);
}

const storedMode = (localStorage.getItem(THEME_KEY) as ThemeMode) || "system";
const storedAccent = (localStorage.getItem(ACCENT_KEY) as Accent) || "default";
const [themeMode, setThemeModeSignal] = createSignal<ThemeMode>(
  THEME_MODES.includes(storedMode) ? storedMode : "system",
);
const [accent, setAccentSignal] = createSignal<Accent>(
  ACCENTS.includes(storedAccent) ? storedAccent : "default",
);

export { themeMode, accent };

// Apply at module load (index.html pre-applies theme to avoid a flash; this
// re-applies the stored theme + accent once the bundle runs).
applyTheme(themeMode());
applyAccent(accent());
// Follow the OS scheme live while in System mode.
if (window.matchMedia) {
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (themeMode() === "system") applyTheme("system");
  });
}

/** Set the color scheme: follow OS, force dark, or force light. */
export function setThemeMode(v: ThemeMode): void {
  setThemeModeSignal(v);
  localStorage.setItem(THEME_KEY, v);
  applyTheme(v);
}

/** Set the accent color (themes `--accent` everywhere via a root attribute). */
export function setAccent(v: Accent): void {
  setAccentSignal(v);
  localStorage.setItem(ACCENT_KEY, v);
  applyAccent(v);
}

export const THEME_LABEL: Record<ThemeMode, string> = { system: "System", dark: "Dark", light: "Light" };
export const ACCENT_LABEL: Record<Accent, string> = { default: "Blue", teal: "Teal" };

// ── Viewport tracking: adaptive layout (phone stacked ↔ tablet side-by-side) ──
// Breakpoints: phone < 768 (narrow/stacked), tablet+ >= 768 (side-by-side),
// "wide" >= 1100 reserved for future tuning. Structural layout switches read
// these reactive signals; spacing/tap-target/safe-area tweaks live in styles.css
// keyed on the data-viewport attribute and @media (pointer: coarse).
const NARROW_MAX = 768;
const WIDE_MIN = 1100;

const [viewportWidth, setViewportWidth] = createSignal<number>(
  typeof window !== "undefined" ? window.innerWidth : 1200,
);
export { viewportWidth };

/** Phone profile: stacked, top-to-bottom flow. */
export const isNarrow = () => viewportWidth() < NARROW_MAX;
/** Extra-wide profile (reserved for future tuning). */
export const isWide = () => viewportWidth() >= WIDE_MIN;

// Coarse pointer (touch) is a fixed media query — sampled once, kept reactive in
// case the OS reports a change (rare).
const coarseMq =
  typeof window !== "undefined" && window.matchMedia
    ? window.matchMedia("(pointer: coarse)")
    : null;
const [coarse, setCoarse] = createSignal<boolean>(coarseMq?.matches ?? false);
/** True on touch devices (used to drop drag handles, grow tap targets). */
export const coarsePointer = () => coarse();
if (coarseMq) coarseMq.addEventListener("change", (e) => setCoarse(e.matches));

/** Hide pane resize handles when dragging is impractical (narrow or touch). */
export const hideResizers = () => isNarrow() || coarsePointer();

/** Reflect the viewport bucket onto <html> so styles.css can drive @media-free
 *  attribute rules (safe-area, tap targets) consistently with the JS signals. */
function applyViewport(w: number): void {
  const bucket = w < NARROW_MAX ? "narrow" : w >= WIDE_MIN ? "wide" : "medium";
  document.documentElement.setAttribute("data-viewport", bucket);
}

if (typeof window !== "undefined") {
  applyViewport(window.innerWidth);
  // Coalesce resize bursts into one update per animation frame.
  let raf = 0;
  window.addEventListener("resize", () => {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      const w = window.innerWidth;
      setViewportWidth(w);
      applyViewport(w);
    });
  });

  // First run with no stored padding AND a coarse pointer → default padding to
  // "l" so the existing ps(...) density grows tap targets, reusing the
  // user-overridable density system instead of hard CSS overrides.
  if (localStorage.getItem(PAD_KEY) === null && (coarseMq?.matches ?? false)) {
    setUiPadding("l");
  }
}
