import { createSignal, onCleanup, onMount } from "solid-js";
import type { Component } from "solid-js";

type Mode = "system" | "dark" | "light";
type Accent = "default" | "teal";

const KEY = "lady-theme";
const ACCENT_KEY = "lady-accent";
const ORDER: Mode[] = ["system", "dark", "light"];
const ACCENTS: Accent[] = ["default", "teal"];
const ICON: Record<Mode, string> = { system: "🖥", dark: "🌙", light: "☀" };
const LABEL: Record<Mode, string> = { system: "System", dark: "Dark", light: "Light" };

const prefersDark = () =>
  window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;

/** Resolve a mode to the concrete light/dark theme and apply it to <html>. */
function apply(mode: Mode) {
  const resolved = mode === "system" ? (prefersDark() ? "dark" : "light") : mode;
  document.documentElement.setAttribute("data-theme", resolved);
}

/** Apply (or clear) the optional accent override on <html>. */
function applyAccent(accent: Accent) {
  if (accent === "default") document.documentElement.removeAttribute("data-accent");
  else document.documentElement.setAttribute("data-accent", accent);
}

/**
 * Theme switcher (top-right): cycles System → Dark → Light, plus an accent
 * toggle that proves the CSS token system (PH6-004) by recoloring `--accent`
 * everywhere without touching any component. Both choices persist in
 * localStorage; System follows the OS `prefers-color-scheme` live.
 */
const ThemeToggle: Component = () => {
  const stored = (localStorage.getItem(KEY) as Mode) || "system";
  const [mode, setMode] = createSignal<Mode>(ORDER.includes(stored) ? stored : "system");
  const storedAccent = (localStorage.getItem(ACCENT_KEY) as Accent) || "default";
  const [accent, setAccent] = createSignal<Accent>(
    ACCENTS.includes(storedAccent) ? storedAccent : "default",
  );

  onMount(() => {
    apply(mode());
    applyAccent(accent());
    // Re-resolve when the OS theme changes while in System mode.
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (mode() === "system") apply("system");
    };
    mq.addEventListener("change", onChange);
    onCleanup(() => mq.removeEventListener("change", onChange));
  });

  const cycle = () => {
    const next = ORDER[(ORDER.indexOf(mode()) + 1) % ORDER.length];
    setMode(next);
    localStorage.setItem(KEY, next);
    apply(next);
  };

  const cycleAccent = () => {
    const next = ACCENTS[(ACCENTS.indexOf(accent()) + 1) % ACCENTS.length];
    setAccent(next);
    localStorage.setItem(ACCENT_KEY, next);
    applyAccent(next);
  };

  const btnStyle = {
    border: "1px solid var(--border)",
    background: "var(--surface)",
    color: "var(--fg)",
    "border-radius": "6px",
    cursor: "pointer",
    "font-size": "0.95rem",
    "line-height": "1",
    padding: "0.25rem 0.45rem",
    display: "flex",
    "align-items": "center",
    gap: "0.3rem",
  };

  return (
    <div style={{ display: "flex", gap: "0.3rem" }}>
      <button
        onClick={cycle}
        title={`Theme: ${LABEL[mode()]} (click to change)`}
        aria-label={`Theme: ${LABEL[mode()]}. Activate to change theme.`}
        style={btnStyle}
      >
        <span aria-hidden="true">{ICON[mode()]}</span>
        <span style={{ "font-size": "0.72rem" }}>{LABEL[mode()]}</span>
      </button>
      <button
        onClick={cycleAccent}
        title={`Accent: ${accent()} (click to change)`}
        aria-label={`Accent color: ${accent()}. Activate to change accent.`}
        aria-pressed={accent() !== "default"}
        style={btnStyle}
      >
        <span
          aria-hidden="true"
          style={{
            width: "0.8rem",
            height: "0.8rem",
            "border-radius": "50%",
            background: "var(--accent)",
            display: "inline-block",
          }}
        />
      </button>
    </div>
  );
};

export default ThemeToggle;
