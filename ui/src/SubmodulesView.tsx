import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { RepoId, Submodule } from "./commands";

/** Join the superproject path with a submodule's relative path. */
const joinPath = (root: string, rel: string): string => {
  const sep = root.includes("\\") ? "\\" : "/";
  return `${root.replace(/[/\\]+$/, "")}${sep}${rel}`;
};

/**
 * Submodules panel (PH4-009): lists submodule status (initialized / dirty) with
 * init / update / sync / remove actions, an add form, and "Open" to open a
 * submodule as its own repo tab (Phase 1 repo manager). Nested submodules are
 * included via `--recursive`.
 */
const SubmodulesView: Component<{
  repoId: RepoId;
  repoPath: string;
  refreshNonce: number;
  onChanged: () => void;
  onOpen: (path: string) => void;
}> = (props) => {
  const [subs, setSubs] = createSignal<Submodule[]>([]);
  const [err, setErr] = createSignal<string | null>(null);
  const [addUrl, setAddUrl] = createSignal("");
  const [addPath, setAddPath] = createSignal("");

  const reload = () => {
    invoke<Submodule[]>("list_submodules", { repo: props.repoId })
      .then(setSubs)
      .catch((e) => setErr(String(e)));
  };
  createEffect(() => {
    props.refreshNonce;
    props.repoId;
    reload();
  });

  const run = (cmd: string, args: Record<string, unknown> = {}) => {
    setErr(null);
    invoke(cmd, { repo: props.repoId, ...args })
      .then(() => {
        reload();
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const add = () => {
    if (!addUrl().trim() || !addPath().trim()) return;
    setErr(null);
    invoke("add_submodule", { repo: props.repoId, url: addUrl().trim(), path: addPath().trim() })
      .then(() => {
        setAddUrl("");
        setAddPath("");
        reload();
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const remove = (path: string) => {
    if (!confirm(`Deinitialize submodule at ${path}?`)) return;
    run("deinit_submodule", { path });
  };

  const smallBtn = {
    border: "1px solid var(--border)",
    background: "var(--surface)",
    "border-radius": "3px",
    "font-size": "0.72rem",
    padding: "0 0.45rem",
    cursor: "pointer",
  };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.85rem 1rem" }}>
      <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "margin-bottom": "0.5rem" }}>
        <h3 style={{ margin: 0, "font-size": "0.95rem" }}>Submodules</h3>
        <button style={smallBtn} onClick={() => run("init_submodules")}>Init all</button>
        <button style={smallBtn} onClick={() => run("update_submodules")}>Update all</button>
        <button style={smallBtn} onClick={() => run("sync_submodules")}>Sync URLs</button>
      </div>

      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.85rem", "white-space": "pre-wrap" }}>{err()}</p>
      </Show>

      <div style={{ display: "flex", gap: "0.4rem", "flex-wrap": "wrap", "align-items": "center", "margin-bottom": "0.6rem" }}>
        <input style={{ flex: "1", "min-width": "14rem", padding: "0.3rem 0.5rem", "font-size": "0.82rem" }} placeholder="submodule url" value={addUrl()} onInput={(e) => setAddUrl(e.currentTarget.value)} />
        <input style={{ width: "10rem", padding: "0.3rem 0.5rem", "font-size": "0.82rem" }} placeholder="path" value={addPath()} onInput={(e) => setAddPath(e.currentTarget.value)} />
        <button onClick={add} style={{ padding: "0.3rem 0.7rem" }}>Add</button>
      </div>

      <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
        <For each={subs()}>
          {(s) => (
            <li style={{ display: "flex", "align-items": "center", gap: "0.5rem", padding: "0.3rem 0", "border-bottom": "1px solid var(--border)", "font-size": "0.83rem" }}>
              <span
                title={!s.initialized ? "not initialized" : s.dirty ? "out of date / modified" : "up to date"}
                style={{ color: !s.initialized ? "var(--fg-muted)" : s.dirty ? "#bc4c00" : "#1a7f37" }}
              >
                {!s.initialized ? "○" : s.dirty ? "◑" : "●"}
              </span>
              <span style={{ flex: "1", "font-family": "monospace", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }} title={s.url}>
                {s.path}
              </span>
              <span style={{ color: "var(--fg-muted)", "font-family": "monospace", "font-size": "0.72rem" }}>{s.sha.slice(0, 8)}</span>
              <Show when={s.initialized}>
                <button style={smallBtn} onClick={() => props.onOpen(joinPath(props.repoPath, s.path))}>Open</button>
              </Show>
              <button style={smallBtn} onClick={() => remove(s.path)}>Remove</button>
            </li>
          )}
        </For>
      </ul>
      <Show when={subs().length === 0 && !err()}>
        <p style={{ color: "var(--fg-muted)", "font-size": "0.85rem" }}>No submodules.</p>
      </Show>
    </div>
  );
};

export default SubmodulesView;
