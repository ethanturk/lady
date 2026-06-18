import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ReflogEntry, RepoId } from "./commands";
import { relTime } from "./time";

const ACTION_COLOR: Record<string, string> = {
  commit: "#1a7f37",
  "commit (amend)": "#1a7f37",
  reset: "#cf222e",
  checkout: "#0969da",
  rebase: "#8250df",
  merge: "#bc4c00",
};

/**
 * Reflog view (PH3-007): lists a ref's reflog newest-first with the action,
 * message, and time. Each entry can be checked out (detached) or branched at,
 * which is how lost commits are recovered.
 */
const ReflogView: Component<{
  repoId: RepoId;
  refreshNonce: number;
  onChanged: () => void;
}> = (props) => {
  const [entries, setEntries] = createSignal<ReflogEntry[]>([]);
  const [refname, setRefname] = createSignal("HEAD");
  const [err, setErr] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);

  const reload = () => {
    invoke<ReflogEntry[]>("reflog", { repo: props.repoId, refname: refname() })
      .then(setEntries)
      .catch((e) => setErr(String(e)));
  };

  createEffect(() => {
    props.refreshNonce;
    props.repoId;
    refname();
    reload();
  });

  const checkoutAt = (oid: string) => {
    setErr(null);
    setNotice(null);
    invoke("checkout", { repo: props.repoId, target: oid, force: false })
      .then(() => {
        setNotice(`Checked out ${oid.slice(0, 8)} (detached).`);
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const branchAt = (oid: string) => {
    const name = prompt(`New branch name at ${oid.slice(0, 8)}:`);
    if (!name) return;
    setErr(null);
    setNotice(null);
    invoke("create_branch", { repo: props.repoId, name, startPoint: oid })
      .then(() => {
        setNotice(`Created branch '${name}' at ${oid.slice(0, 8)}.`);
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
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
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.75rem 1rem" }}>
      <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "margin-bottom": "0.5rem" }}>
        <h3 style={{ margin: 0, "font-size": "0.95rem" }}>Reflog</h3>
        <input
          style={{ padding: "0.2rem 0.4rem", "font-size": "0.8rem", width: "10rem" }}
          value={refname()}
          onInput={(e) => setRefname(e.currentTarget.value)}
          placeholder="HEAD or a ref"
          title="Reflog of which ref"
        />
      </div>

      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.85rem" }}>{err()}</p>
      </Show>
      <Show when={notice()}>
        <p style={{ color: "#1a7f37", "font-size": "0.85rem" }}>{notice()}</p>
      </Show>

      <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
        <For each={entries()}>
          {(e, i) => (
            <li
              style={{
                display: "flex",
                "align-items": "center",
                gap: "0.5rem",
                padding: "0.3rem 0",
                "border-bottom": "1px solid var(--border)",
                "font-size": "0.83rem",
              }}
            >
              <span style={{ "font-family": "monospace", color: "var(--fg-muted)", "min-width": "9ch" }}>
                {`${refname()}@{${i()}}`}
              </span>
              <span style={{ "font-family": "monospace", color: "var(--fg-muted)", "min-width": "8ch" }}>
                {e.oid.slice(0, 8)}
              </span>
              <span
                style={{
                  color: ACTION_COLOR[e.action] ?? "var(--fg)",
                  "font-weight": 600,
                  "min-width": "6ch",
                }}
              >
                {e.action}
              </span>
              <span style={{ flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }} title={e.message}>
                {e.message}
              </span>
              <span style={{ color: "var(--fg-muted)", "white-space": "nowrap" }}>{relTime(e.time)}</span>
              <button style={smallBtn} onClick={() => branchAt(e.oid)}>
                Branch
              </button>
              <button style={smallBtn} onClick={() => checkoutAt(e.oid)}>
                Checkout
              </button>
            </li>
          )}
        </For>
      </ul>
    </div>
  );
};

export default ReflogView;
