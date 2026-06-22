import { createSignal, Show } from "solid-js";
import type { Component } from "solid-js";
import type { RepoId } from "./commands";
import { invoke } from "@tauri-apps/api/core";

export interface PushDialogState {
  repo: RepoId;
  /** The local ref to push (branch name or full ref like refs/tags/v1). */
  refspec: string;
  /** The remote name (defaults to origin if empty). */
  remote: string;
  /** Whether this is a tag push (disables upstream tracking). */
  isTag: boolean;
  /** Called after a successful push so the host can refresh. */
  onSuccess: () => void;
  /** Called with an error message on failure. */
  onError: (msg: string) => void;
}

/**
 * Push confirmation dialog shown for every push operation. Lets the user choose
 * the remote and opt into a force push before the network request is made.
 */
const PushDialog: Component<{ state: PushDialogState; onClose: () => void }> = (props) => {
  const [remote, setRemote] = createSignal(props.state.remote || "origin");
  const [force, setForce] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  const label = () => (props.state.isTag ? `tag ${props.state.refspec}` : `branch ${props.state.refspec}`);

  const submit = async () => {
    setBusy(true);
    try {
      await invoke("push", {
        repo: props.state.repo,
        remote: remote(),
        branch: props.state.refspec,
        setUpstream: !props.state.isTag,
        force: force(),
      });
      props.onClose();
      props.state.onSuccess();
    } catch (e) {
      props.state.onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      style={{ position: "fixed", inset: "0", background: "rgba(0,0,0,0.35)", display: "flex", "align-items": "flex-start", "justify-content": "center", "padding-top": "18vh", "z-index": "1000" }}
      onClick={props.onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{ width: "min(420px, 90vw)", background: "var(--pill)", border: "1px solid var(--bd)", "border-radius": "9px", padding: "16px", "box-shadow": "0 14px 38px rgba(0,0,0,0.45)", display: "flex", "flex-direction": "column", gap: "10px" }}
      >
        <div style={{ "font-size": "13px", color: "var(--tx2)" }}>
          Push {label()} to remote
        </div>
        <input
          value={remote()}
          onInput={(e) => setRemote(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !busy()) submit();
            if (e.key === "Escape") props.onClose();
          }}
          placeholder="remote name"
          style={{ background: "var(--input)", border: "1px solid var(--bd)", "border-radius": "7px", color: "var(--tx)", "font-size": "13px", padding: "9px 12px" }}
        />
        <label style={{ display: "flex", "align-items": "center", gap: "6px", color: "var(--tx)", "font-size": "13px", cursor: "pointer" }}>
          <input
            type="checkbox"
            checked={force()}
            onChange={(e) => setForce(e.currentTarget.checked)}
          />
          Force push
        </label>
        <Show when={force()}>
          <p style={{ margin: 0, color: "var(--warning)", "font-size": "12px" }}>
            Force push overwrites remote history and can lose commits. Use with care.
          </p>
        </Show>
        <div style={{ display: "flex", "justify-content": "flex-end", gap: "8px" }}>
          <button onClick={props.onClose} disabled={busy()} style={{ border: "1px solid var(--bd)", background: "var(--btn)", color: "var(--tx)", "border-radius": "7px", padding: "7px 16px", cursor: "pointer", "font-size": "12.5px" }}>
            Cancel
          </button>
          <button
            onClick={submit}
            disabled={busy() || !remote().trim()}
            style={{ border: "none", background: "var(--accent)", color: "var(--on-accent-strong)", "border-radius": "7px", padding: "7px 16px", cursor: "pointer", "font-size": "12.5px", "font-weight": 600 }}
          >
            {busy() ? "Pushing…" : "Push"}
          </button>
        </div>
      </div>
    </div>
  );
};

export default PushDialog;
