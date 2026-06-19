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

export {
  sidebarWidth,
  setSidebarWidth,
  changesColWidth,
  setChangesColWidth,
  settingsWidth,
  setSettingsWidth,
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
