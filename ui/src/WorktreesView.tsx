import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { RepoId, Worktree } from "./commands";

/**
 * Worktrees panel (PH3-006): lists the repo's worktrees with add (path +
 * branch / new-branch) and remove. Opening one hands its path to the repo
 * manager so it becomes a tab.
 */
const WorktreesView: Component<{
  repoId: RepoId;
  refreshNonce: number;
  onChanged: () => void;
  onOpen: (path: string) => void;
}> = (props) => {
  const [worktrees, setWorktrees] = createSignal<Worktree[]>([]);
  const [err, setErr] = createSignal<string | null>(null);
  const [path, setPath] = createSignal("");
  const [branch, setBranch] = createSignal("");
  const [newBranch, setNewBranch] = createSignal(true);

  const reload = () => {
    invoke<Worktree[]>("list_worktrees", { repo: props.repoId })
      .then(setWorktrees)
      .catch((e) => setErr(String(e)));
  };

  createEffect(() => {
    props.refreshNonce;
    props.repoId;
    reload();
  });

  const add = () => {
    if (!path()) return;
    setErr(null);
    invoke("add_worktree", {
      repo: props.repoId,
      path: path(),
      branch: branch() || null,
      newBranch: newBranch(),
    })
      .then(() => {
        setPath("");
        setBranch("");
        reload();
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const remove = (wtPath: string) => {
    if (!confirm(`Remove worktree at ${wtPath}?`)) return;
    setErr(null);
    invoke("remove_worktree", { repo: props.repoId, path: wtPath })
      .then(() => {
        reload();
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const prune = () => {
    invoke("prune_worktrees", { repo: props.repoId })
      .then(() => {
        reload();
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const inputStyle = { padding: "0.3rem 0.5rem", "font-size": "0.85rem" };
  const smallBtn = {
    border: "1px solid var(--border)",
    background: "var(--surface)",
    "border-radius": "3px",
    "font-size": "0.72rem",
    padding: "0 0.45rem",
    cursor: "pointer",
  };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.75rem 1rem" }}>
      <h3 style={{ margin: "0 0 0.5rem", "font-size": "0.95rem" }}>Worktrees</h3>

      <div style={{ display: "flex", gap: "0.4rem", "flex-wrap": "wrap", "align-items": "center", "margin-bottom": "0.6rem" }}>
        <input
          style={{ ...inputStyle, flex: "1", "min-width": "14rem" }}
          placeholder="/path/to/new/worktree"
          value={path()}
          onInput={(e) => setPath(e.currentTarget.value)}
        />
        <input
          style={{ ...inputStyle, width: "10rem" }}
          placeholder={newBranch() ? "new-branch-name" : "existing-branch"}
          value={branch()}
          onInput={(e) => setBranch(e.currentTarget.value)}
        />
        <label style={{ display: "flex", "align-items": "center", gap: "0.25rem", "font-size": "0.8rem" }}>
          <input type="checkbox" checked={newBranch()} onChange={() => setNewBranch((v) => !v)} />
          new branch
        </label>
        <button onClick={add} style={{ padding: "0.3rem 0.7rem" }}>
          Add
        </button>
        <button onClick={prune} style={{ padding: "0.3rem 0.7rem" }} title="Prune stale worktree entries">
          Prune
        </button>
      </div>

      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
        <For each={worktrees()}>
          {(wt) => (
            <li
              style={{
                display: "flex",
                "align-items": "center",
                gap: "0.5rem",
                padding: "0.3rem 0",
                "border-bottom": "1px solid var(--border)",
                "font-size": "0.85rem",
              }}
            >
              <span style={{ "font-family": "monospace", flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }} title={wt.path}>
                {wt.path}
              </span>
              <span style={{ color: "var(--info)", "min-width": "8ch" }}>
                {wt.branch ?? "(detached)"}
              </span>
              <span style={{ "font-family": "monospace", color: "var(--fg-muted)" }}>
                {wt.head ? wt.head.slice(0, 8) : "—"}
              </span>
              <Show when={wt.locked}>
                <span title="locked" style={{ color: "var(--warning)" }}>🔒</span>
              </Show>
              <button style={smallBtn} onClick={() => props.onOpen(wt.path)}>
                Open
              </button>
              <button style={smallBtn} onClick={() => remove(wt.path)}>
                Remove
              </button>
            </li>
          )}
        </For>
      </ul>
    </div>
  );
};

export default WorktreesView;
