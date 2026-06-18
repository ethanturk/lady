import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { LfsStatus, RepoId } from "./commands";

/**
 * Git LFS panel (PH4-007): shows availability, tracked patterns, and tracked
 * files with their materialized (●) vs pointer-only (○) state, plus a
 * "Track pattern with LFS" action. Clone/fetch/checkout already run smudge/clean
 * filters because every git op shells out to the user's git.
 */
const LfsView: Component<{
  repoId: RepoId;
  refreshNonce: number;
  onChanged: () => void;
}> = (props) => {
  const [status, setStatus] = createSignal<LfsStatus>({ available: false, patterns: [], files: [] });
  const [pattern, setPattern] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);

  const reload = () => {
    invoke<LfsStatus>("lfs_status", { repo: props.repoId })
      .then(setStatus)
      .catch((e) => setErr(String(e)));
  };
  createEffect(() => {
    props.refreshNonce;
    props.repoId;
    reload();
  });

  const track = () => {
    if (!pattern().trim()) return;
    setErr(null);
    invoke("lfs_track", { repo: props.repoId, pattern: pattern().trim() })
      .then(() => {
        setPattern("");
        reload();
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.85rem 1rem" }}>
      <h3 style={{ margin: "0 0 0.5rem", "font-size": "0.95rem" }}>Git LFS</h3>

      <Show
        when={status().available}
        fallback={
          <p style={{ color: "#9a6700", background: "#fff8c5", padding: "0.4rem 0.6rem", "border-radius": "4px", "font-size": "0.85rem" }}>
            git-lfs is not installed. Install it from{" "}
            <span style={{ "font-family": "monospace" }}>git-lfs.com</span> to track large files.
          </p>
        }
      >
        <Show when={err()}>
          <p style={{ color: "crimson", "font-size": "0.85rem" }}>{err()}</p>
        </Show>

        <div style={{ display: "flex", gap: "0.4rem", "align-items": "center", "margin-bottom": "0.6rem" }}>
          <input
            style={{ flex: "1", "max-width": "18rem", padding: "0.3rem 0.5rem", "font-family": "monospace", "font-size": "0.82rem" }}
            placeholder="*.psd"
            value={pattern()}
            onInput={(e) => setPattern(e.currentTarget.value)}
            onKeyDown={(e) => e.key === "Enter" && track()}
          />
          <button onClick={track} style={{ padding: "0.3rem 0.8rem" }}>
            Track pattern with LFS
          </button>
        </div>

        <h4 style={{ margin: "0.6rem 0 0.3rem", "font-size": "0.85rem" }}>Tracked patterns</h4>
        <Show when={status().patterns.length > 0} fallback={<p style={{ color: "#888", "font-size": "0.82rem" }}>None.</p>}>
          <ul style={{ margin: 0, padding: "0 0 0 1.1rem", "font-family": "monospace", "font-size": "0.82rem" }}>
            <For each={status().patterns}>{(p) => <li>{p}</li>}</For>
          </ul>
        </Show>

        <h4 style={{ margin: "0.8rem 0 0.3rem", "font-size": "0.85rem" }}>Tracked files</h4>
        <Show when={status().files.length > 0} fallback={<p style={{ color: "#888", "font-size": "0.82rem" }}>None.</p>}>
          <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
            <For each={status().files}>
              {(f) => (
                <li style={{ display: "flex", "align-items": "center", gap: "0.5rem", padding: "0.2rem 0", "font-size": "0.83rem" }}>
                  <span title={f.downloaded ? "materialized" : "pointer only"} style={{ color: f.downloaded ? "#1a7f37" : "#999" }}>
                    {f.downloaded ? "●" : "○"}
                  </span>
                  <span style={{ flex: "1", "font-family": "monospace", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
                    {f.path}
                  </span>
                  <span style={{ color: "#888", "font-family": "monospace", "font-size": "0.72rem" }}>{f.oid.slice(0, 8)}</span>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </div>
  );
};

export default LfsView;
