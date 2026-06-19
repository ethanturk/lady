import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { RepoId, StashEntry } from "./commands";
import { cancelAi, isConsentError, runAiStream } from "./ai";

const subBtn = {
  border: "1px solid var(--bd)",
  background: "var(--btn)",
  color: "var(--tx)",
  "border-radius": "6px",
  "font-size": "12px",
  padding: "4px 14px",
  cursor: "pointer",
} as const;

const field = {
  background: "var(--input)",
  border: "1px solid var(--bd)",
  "border-radius": "7px",
  color: "var(--tx)",
  "font-family": "inherit",
  "font-size": "12.5px",
  padding: "7px 10px",
} as const;

interface StashViewProps {
  repoId: RepoId;
  /** Bump to reload the stash list after an external mutation. */
  refreshNonce?: number;
  /** Called after a stash mutation so sibling views can reload. */
  onChanged?: () => void;
}

/**
 * Stash management (moved out of the Local Changes file column): create a stash
 * of the working tree (optionally with an AI-generated note and untracked
 * files), and apply / pop / drop existing stashes.
 */
const StashView: Component<StashViewProps> = (props) => {
  const [stashes, setStashes] = createSignal<StashEntry[]>([]);
  const [stashMsg, setStashMsg] = createSignal("");
  const [stashUntracked, setStashUntracked] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const [aiBusy, setAiBusy] = createSignal(false);
  const [aiReq, setAiReq] = createSignal<string | null>(null);

  const reload = () => {
    invoke<StashEntry[]>("stash_list", { repo: props.repoId }).then(setStashes).catch(() => setStashes([]));
  };
  createEffect(() => {
    void props.repoId;
    void props.refreshNonce;
    reload();
  });

  const afterMutation = () => {
    reload();
    props.onChanged?.();
  };

  const stashSave = () => {
    const msg = stashMsg().trim();
    invoke("stash_save", { repo: props.repoId, message: msg === "" ? null : msg, includeUntracked: stashUntracked() })
      .then(() => setStashMsg(""))
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };

  const generateStashNote = async () => {
    if (aiBusy()) return;
    setErr(null);
    setAiBusy(true);
    setStashMsg("");
    try {
      const full = await runAiStream("ai_stash_note", { repo: props.repoId }, (acc) => setStashMsg(acc), (id) => setAiReq(id));
      setStashMsg(full.trim());
    } catch (e) {
      const msg = String(e);
      setErr(isConsentError(msg) ? "AI consent required — enable the provider and grant consent in Settings." : msg);
    } finally {
      setAiBusy(false);
      setAiReq(null);
    }
  };
  const cancelGenerate = () => {
    const id = aiReq();
    if (id) cancelAi(id).catch(() => {});
  };

  const stashOp = (cmd: string, index: number) => {
    invoke(cmd, { repo: props.repoId, index }).then(afterMutation).catch((e) => setErr(String(e)));
  };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "16px 18px", "max-width": "44rem" }}>
      <h3 style={{ margin: "0 0 0.6rem", "font-size": "0.95rem" }}>Stashes</h3>

      <Show when={err()}>
        <p role="alert" style={{ color: "var(--error)", "font-size": "12.5px", margin: "0 0 8px" }}>{err()}</p>
      </Show>

      {/* Create a stash of the current working tree. */}
      <div style={{ display: "flex", "flex-direction": "column", gap: "8px", "max-width": "32rem", "margin-bottom": "16px" }}>
        <div style={{ display: "flex", gap: "6px" }}>
          <input
            style={{ ...field, flex: "1" }}
            placeholder="stash note (optional)…"
            value={stashMsg()}
            onInput={(e) => setStashMsg(e.currentTarget.value)}
          />
          <button style={subBtn} disabled={aiBusy()} title="Generate a stash note with AI" onClick={generateStashNote}>{aiBusy() ? "…" : "✨"}</button>
          <Show when={aiBusy()}>
            <button style={subBtn} onClick={cancelGenerate}>Cancel</button>
          </Show>
        </div>
        <div style={{ display: "flex", "align-items": "center", gap: "10px" }}>
          <button style={subBtn} onClick={stashSave}>Stash changes</button>
          <label style={{ display: "flex", "align-items": "center", gap: "5px", "font-size": "12px", color: "var(--tx3)" }}>
            <input type="checkbox" checked={stashUntracked()} onChange={(e) => setStashUntracked(e.currentTarget.checked)} />
            include untracked
          </label>
        </div>
      </div>

      {/* Existing stashes. */}
      <Show
        when={stashes().length > 0}
        fallback={<p style={{ color: "var(--tx3)", "font-size": "12.5px" }}>No stashes.</p>}
      >
        <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
          <For each={stashes()}>
            {(s) => (
              <div style={{ display: "flex", "align-items": "center", gap: "8px", "font-size": "12.5px", padding: "6px 4px", "border-bottom": "1px solid var(--bd)" }}>
                <span style={{ color: "var(--accent-2)", "font-family": "ui-monospace, monospace" }}>{`stash@{${s.index}}`}</span>
                <span style={{ flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }} title={s.message}>{s.message}</span>
                <button style={{ ...subBtn, padding: "3px 10px" }} onClick={() => stashOp("stash_apply", s.index)}>Apply</button>
                <button style={{ ...subBtn, padding: "3px 10px" }} onClick={() => stashOp("stash_pop", s.index)}>Pop</button>
                <button style={{ ...subBtn, padding: "3px 10px" }} onClick={() => confirm(`Drop stash@{${s.index}}?`) && stashOp("stash_drop", s.index)}>Drop</button>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default StashView;
