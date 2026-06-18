import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AheadBehind, RepoId } from "./commands";

interface SyncBarProps {
  repoId: RepoId;
  /** Bumped by the parent after any mutation so ahead/behind re-reads. */
  refreshNonce: number;
  /** Called after fetch/pull/push so the parent reloads refs + status. */
  onChanged: () => void;
}

const btn = {
  border: "1px solid #ccc",
  background: "#fff",
  "border-radius": "4px",
  "font-size": "0.8rem",
  padding: "0.2rem 0.6rem",
  cursor: "pointer",
};

/**
 * Sync controls: Fetch / Pull / Push plus an ahead/behind indicator. Long
 * network ops stream git's own `--progress` output (fetch-progress /
 * push-progress events) into a status line; failures surface git's message
 * verbatim. Auth is entirely the system git's (ADR-0006) — nothing is gated.
 */
const SyncBar: Component<SyncBarProps> = (props) => {
  const [ab, setAb] = createSignal<AheadBehind | null>(null);
  const [busy, setBusy] = createSignal<string | null>(null);
  const [progress, setProgress] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);

  // Stream progress from both fetch and push into the same status line.
  const unlisten: Array<() => void> = [];
  listen<string>("fetch-progress", (e) => setProgress(e.payload)).then((u) =>
    unlisten.push(u),
  );
  listen<string>("push-progress", (e) => setProgress(e.payload)).then((u) =>
    unlisten.push(u),
  );
  onCleanup(() => unlisten.forEach((u) => u()));

  const loadAheadBehind = () => {
    invoke<AheadBehind | null>("ahead_behind", { repo: props.repoId })
      .then(setAb)
      .catch(() => setAb(null));
  };

  // Re-read ahead/behind on repo change and after any parent-signalled mutation.
  createEffect(() => {
    props.repoId;
    props.refreshNonce;
    loadAheadBehind();
  });

  const run = async (label: string, cmd: string, args: Record<string, unknown>) => {
    setErr(null);
    setProgress("");
    setBusy(label);
    try {
      await invoke(cmd, { repo: props.repoId, ...args });
      props.onChanged();
      loadAheadBehind();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  };

  const fetch = () => run("Fetching", "fetch", { remote: null });
  const pull = () => run("Pulling", "pull", { remote: null, branch: null });
  const push = (setUpstream = false, force = false) =>
    run("Pushing", "push", { remote: null, branch: null, setUpstream, force });

  // No upstream yet → first push must set it; otherwise git rejects with a hint.
  const pushSafely = () => {
    if (ab() === null) {
      push(true, false);
      return;
    }
    push(false, false);
  };

  return (
    <div style={{ "flex-shrink": 0, padding: "0.25rem 1rem" }}>
      <div style={{ display: "flex", "align-items": "center", gap: "0.4rem" }}>
        <button style={btn} disabled={!!busy()} onClick={fetch}>
          Fetch
        </button>
        <button style={btn} disabled={!!busy()} onClick={pull}>
          Pull
        </button>
        <button style={btn} disabled={!!busy()} onClick={pushSafely}>
          Push
        </button>

        <Show when={ab()}>
          {(v) => (
            <span style={{ "font-size": "0.8rem", color: "#444", "font-family": "monospace" }}>
              ↑{v().ahead} ↓{v().behind}
            </span>
          )}
        </Show>
        <Show when={ab() === null}>
          <span style={{ "font-size": "0.75rem", color: "#999" }}>no upstream</span>
        </Show>

        <Show when={busy()}>
          <span style={{ "font-size": "0.8rem", color: "#0070f3" }}>{busy()}…</span>
        </Show>
      </div>

      <Show when={busy() && progress()}>
        <p style={{ margin: "0.15rem 0 0", "font-size": "0.72rem", color: "#666", "font-family": "monospace" }}>
          {progress()}
        </p>
      </Show>
      <Show when={err()}>
        <p style={{ margin: "0.15rem 0 0", color: "crimson", "font-size": "0.78rem", "white-space": "pre-wrap" }}>
          {err()}
        </p>
      </Show>
    </div>
  );
};

export default SyncBar;
