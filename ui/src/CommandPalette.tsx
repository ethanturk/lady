import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Component } from "solid-js";

export interface PaletteEntry {
  /** Category shown as a dim prefix in the row. */
  kind: "action" | "branch" | "file";
  label: string;
  run: () => void;
}

/**
 * Subsequence fuzzy match: every char of `query` must appear in `text` in
 * order. Returns a score (lower = better, contiguous + early matches win) or
 * `null` when there's no match. Empty query matches everything.
 */
function fuzzyScore(query: string, text: string): number | null {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  let ti = 0;
  let score = 0;
  let prev = -1;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found === -1) return null;
    // Penalize gaps between matched chars; reward contiguity.
    score += found - (prev + 1);
    prev = found;
    ti = found + 1;
  }
  // Bias toward shorter targets so exact-ish matches float up.
  return score + t.length * 0.01;
}

const KIND_COLOR: Record<PaletteEntry["kind"], string> = {
  action: "var(--accent)",
  branch: "#0a8a0a",
  file: "#8a0a8a",
};

const CommandPalette: Component<{
  open: boolean;
  entries: PaletteEntry[];
  onOpen: () => void;
  onClose: () => void;
}> = (props) => {
  const [query, setQuery] = createSignal("");
  const [cursor, setCursor] = createSignal(0);

  const results = createMemo(() => {
    const q = query();
    const scored = props.entries
      .map((e) => ({ e, s: fuzzyScore(q, e.label) }))
      .filter((x): x is { e: PaletteEntry; s: number } => x.s !== null)
      .sort((a, b) => a.s - b.s)
      .slice(0, 50)
      .map((x) => x.e);
    return scored;
  });

  // Global Cmd/Ctrl+P toggles the palette.
  onMount(() => {
    const onKey = (ev: KeyboardEvent) => {
      if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === "p") {
        ev.preventDefault();
        if (props.open) props.onClose();
        else props.onOpen();
      }
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  // Reset query/cursor each time it opens.
  createEffect(() => {
    if (props.open) {
      setQuery("");
      setCursor(0);
    }
  });

  const choose = (entry: PaletteEntry | undefined) => {
    if (!entry) return;
    entry.run();
    props.onClose();
  };

  const onInputKey = (ev: KeyboardEvent) => {
    const list = results();
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      setCursor((c) => Math.min(c + 1, list.length - 1));
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      setCursor((c) => Math.max(c - 1, 0));
    } else if (ev.key === "Enter") {
      ev.preventDefault();
      choose(list[cursor()]);
    } else if (ev.key === "Escape") {
      ev.preventDefault();
      props.onClose();
    }
  };

  return (
    <Show when={props.open}>
      <div
        onClick={props.onClose}
        style={{
          position: "fixed",
          inset: "0",
          background: "rgba(0,0,0,0.3)",
          display: "flex",
          "justify-content": "center",
          "align-items": "flex-start",
          "padding-top": "10vh",
          "z-index": "1000",
        }}
      >
        <div
          onClick={(e) => e.stopPropagation()}
          style={{
            width: "min(640px, 90vw)",
            background: "var(--surface)",
            "border-radius": "8px",
            "box-shadow": "0 12px 40px rgba(0,0,0,0.3)",
            overflow: "hidden",
            display: "flex",
            "flex-direction": "column",
          }}
        >
          <input
            ref={(el) => queueMicrotask(() => el.focus())}
            type="text"
            value={query()}
            onInput={(e) => {
              setQuery(e.currentTarget.value);
              setCursor(0);
            }}
            onKeyDown={onInputKey}
            placeholder="Jump to action, branch, or file…"
            style={{
              padding: "0.7rem 0.9rem",
              "font-size": "1rem",
              border: "none",
              "border-bottom": "1px solid var(--border)",
              outline: "none",
            }}
          />
          <div style={{ "max-height": "50vh", "overflow-y": "auto" }}>
            <Show
              when={results().length > 0}
              fallback={
                <div style={{ padding: "0.8rem", color: "var(--fg-muted)", "font-size": "0.85rem" }}>
                  No matches.
                </div>
              }
            >
              <For each={results()}>
                {(entry, i) => (
                  <div
                    onClick={() => choose(entry)}
                    onMouseEnter={() => setCursor(i())}
                    style={{
                      display: "flex",
                      "align-items": "center",
                      gap: "0.6rem",
                      padding: "0.45rem 0.9rem",
                      cursor: "pointer",
                      background: cursor() === i() ? "var(--selection)" : "transparent",
                      "font-size": "0.88rem",
                    }}
                  >
                    <span
                      style={{
                        color: KIND_COLOR[entry.kind],
                        "font-size": "0.7rem",
                        "text-transform": "uppercase",
                        "min-width": "3.5rem",
                      }}
                    >
                      {entry.kind}
                    </span>
                    <span
                      style={{
                        flex: "1",
                        overflow: "hidden",
                        "text-overflow": "ellipsis",
                        "white-space": "nowrap",
                        "font-family": entry.kind === "action" ? "inherit" : "monospace",
                      }}
                    >
                      {entry.label}
                    </span>
                  </div>
                )}
              </For>
            </Show>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default CommandPalette;
