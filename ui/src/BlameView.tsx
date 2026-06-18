import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { Blame, RepoId } from "./commands";

/** Stable-ish color per commit oid so blame gutters group visually. */
function commitColor(oid: string): string {
  let h = 0;
  for (let i = 0; i < 8 && i < oid.length; i++) h = (h * 31 + oid.charCodeAt(i)) % 360;
  return `hsl(${h}, 45%, 88%)`;
}

const BlameView: Component<{ repoId: RepoId; initialPath?: string }> = (props) => {
  const [file, setFile] = createSignal("");
  const [blame, setBlame] = createSignal<Blame | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);

  const run = async () => {
    if (!file()) return;
    setLoading(true);
    setErr(null);
    setBlame(null);
    try {
      const b = await invoke<Blame>("blame", { repo: props.repoId, path: file() });
      setBlame(b);
    } catch (e) {
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  };

  // When the palette navigates here with a path, load it automatically.
  createEffect(() => {
    const p = props.initialPath;
    if (p && p !== file()) {
      setFile(p);
      run();
    }
  });

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column" }}>
      <div style={{ display: "flex", gap: "0.5rem", padding: "0.5rem", "flex-shrink": 0 }}>
        <input
          type="text"
          value={file()}
          onInput={(e) => setFile(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") run();
          }}
          placeholder="repo-relative path, e.g. src/main.rs"
          style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
        />
        <button onClick={run} style={{ padding: "0.3rem 0.8rem" }}>
          Blame
        </button>
      </div>
      <div style={{ flex: "1", "overflow-y": "auto", "font-family": "monospace", "font-size": "0.8rem" }}>
        <Show when={err()}>
          <p style={{ color: "var(--error)", padding: "0.5rem" }}>{err()}</p>
        </Show>
        <Show when={loading()}>
          <p style={{ color: "var(--fg-muted)", padding: "0.5rem" }}>Loading blame…</p>
        </Show>
        <For each={blame()?.lines ?? []}>
          {(line) => (
            <div style={{ display: "flex", "align-items": "stretch" }}>
              <span
                title={`${line.author} • ${new Date(line.time * 1000).toLocaleDateString()}`}
                style={{
                  background: commitColor(line.commit),
                  // Gutter bg is always a light pastel → use dark text in both
                  // themes so it stays readable.
                  color: "var(--on-light)",
                  padding: "0 0.4rem",
                  "white-space": "nowrap",
                  "border-right": "1px solid var(--border)",
                  "min-width": "16ch",
                  overflow: "hidden",
                  "text-overflow": "ellipsis",
                }}
              >
                {line.commit.slice(0, 8)} {line.author}
              </span>
              <span
                style={{
                  color: "var(--fg-muted)",
                  padding: "0 0.4rem",
                  "text-align": "right",
                  "min-width": "4ch",
                  "user-select": "none",
                }}
              >
                {line.line_no}
              </span>
              <span style={{ "white-space": "pre", padding: "0 0.4rem", flex: "1" }}>
                {line.content || " "}
              </span>
            </div>
          )}
        </For>
      </div>
    </div>
  );
};

export default BlameView;
