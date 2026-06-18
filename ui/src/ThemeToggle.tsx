import { createSignal, onCleanup, onMount } from "solid-js";
import type { Component } from "solid-js";

type Mode = "system" | "dark" | "light";

const KEY = "lady-theme";
const ORDER: Mode[] = ["system", "dark", "light"];
const ICON: Record<Mode, string> = { system: "🖥", dark: "🌙", light: "☀" };
const LABEL: Record<Mode, string> = { system: "System", dark: "Dark", light: "Light" };

const prefersDark = () =>
  window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;

/** Resolve a mode to the concrete light/dark theme and apply it to <html>. */
function apply(mode: Mode) {
  const resolved = mode === "system" ? (prefersDark() ? "dark" : "light") : mode;
  document.documentElement.setAttribute("data-theme", resolved);
}

/**
 * Theme switcher (top-right): cycles System → Dark → Light. Choice persists in
 * localStorage; System follows the OS `prefers-color-scheme` live.
 */
const ThemeToggle: Component = () => {
  const stored = (localStorage.getItem(KEY) as Mode) || "system";
  const [mode, setMode] = createSignal<Mode>(ORDER.includes(stored) ? stored : "system");

  onMount(() => {
    apply(mode());
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

  return (
    <button
      onClick={cycle}
      title={`Theme: ${LABEL[mode()]} (click to change)`}
      style={{
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
      }}
    >
      <span aria-hidden="true">{ICON[mode()]}</span>
      <span style={{ "font-size": "0.72rem" }}>{LABEL[mode()]}</span>
    </button>
  );
};

export default ThemeToggle;
