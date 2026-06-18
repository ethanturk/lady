import { createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { CommitMeta, RepoId } from "./commands";
import { relTime } from "./time";
import DiffView from "./DiffView";

const FileHistory: Component<{ repoId: RepoId }> = (props) => {
  const [file, setFile] = createSignal("");
  const [commits, setCommits] = createSignal<CommitMeta[]>([]);
  const [selected, setSelected] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);

  const run = async () => {
    if (!file()) return;
    setLoading(true);
    setErr(null);
    setCommits([]);
    setSelected(null);
    try {
      const list = await invoke<CommitMeta[]>("file_history", {
        repo: props.repoId,
        path: file(),
      });
      setCommits(list);
    } catch (e) {
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  };

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
          History
        </button>
      </div>
      <div style={{ flex: "1", display: "flex", overflow: "hidden" }}>
        <div style={{ flex: "1", "min-width": "0", "overflow-y": "auto", "border-right": "1px solid var(--border)" }}>
          <Show when={err()}>
            <p style={{ color: "var(--error)", padding: "0.5rem", "font-size": "0.85rem" }}>{err()}</p>
          </Show>
          <Show when={loading()}>
            <p style={{ color: "var(--fg-muted)", padding: "0.5rem", "font-size": "0.85rem" }}>Loading…</p>
          </Show>
          <Show when={!loading() && commits().length === 0 && !err() && file()}>
            <p style={{ color: "var(--fg-muted)", padding: "0.5rem", "font-size": "0.85rem" }}>
              No history for this path.
            </p>
          </Show>
          <For each={commits()}>
            {(c) => (
              <div
                onClick={() => setSelected(c.oid)}
                style={{
                  padding: "0.4rem 0.6rem",
                  "border-bottom": "1px solid var(--border)",
                  cursor: "pointer",
                  "font-size": "0.85rem",
                  background: selected() === c.oid ? "var(--selection)" : "transparent",
                }}
              >
                <div style={{ display: "flex", gap: "0.5rem" }}>
                  <span style={{ "font-family": "monospace", color: "var(--fg-muted)" }}>
                    {c.oid.slice(0, 8)}
                  </span>
                  <span style={{ flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
                    {c.summary}
                  </span>
                </div>
                <div style={{ color: "var(--fg-muted)", "font-size": "0.75rem" }}>
                  {c.author.name} • {relTime(c.time)}
                </div>
              </div>
            )}
          </For>
        </div>
        <Show when={selected()}>
          <div style={{ flex: "1.4", "min-width": "0", overflow: "hidden" }}>
            <DiffView repoId={props.repoId} commit={selected()!} filterPath={file()} />
          </div>
        </Show>
      </div>
    </div>
  );
};

export default FileHistory;
