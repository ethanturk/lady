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

  const shortRef = () => props.state.refspec.replace(/^refs\/tags\//, "");
  const branchLabel = () => (props.state.isTag ? `tag ${shortRef()}` : shortRef());
  const destinationLabel = () => {
    const r = remote().trim() || "origin";
    if (props.state.isTag) return `default (${r})`;
    return `default (${r}/${shortRef()})`;
  };

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

  const backdrop = {
    position: "fixed",
    inset: "0",
    background: "rgba(0,0,0,0.42)",
    display: "flex",
    "align-items": "flex-start",
    "justify-content": "center",
    "padding-top": "calc(16vh + env(safe-area-inset-top, 0px))",
    "z-index": "1000",
  } as const;

  const dialog = {
    width: "min(494px, 94vw)",
    background: "var(--pill)",
    border: "1px solid var(--bds)",
    "border-radius": "24px",
    padding: "22px 18px 16px",
    "box-shadow": "0 18px 54px rgba(0,0,0,0.46)",
    display: "grid",
    "grid-template-columns": "66px minmax(0, 1fr)",
    gap: "0 18px",
  } as const;

  const icon = {
    width: "56px",
    height: "56px",
    "border-radius": "50%",
    background: "linear-gradient(180deg, #21b7ef 0%, #0c83c7 100%)",
    border: "2px solid #ffffff",
    "box-shadow": "0 0 0 1px rgba(0,0,0,0.22), inset 0 -10px 18px rgba(0,0,0,0.12)",
    display: "grid",
    "place-items": "center",
    "align-self": "start",
  } as const;

  const form = {
    display: "grid",
    "grid-template-columns": "56px minmax(0, 1fr)",
    gap: "8px 8px",
    "align-items": "center",
  } as const;

  const labelStyle = {
    color: "var(--tx2)",
    "font-size": "13px",
    "text-align": "right",
  } as const;

  const selectLike = {
    width: "100%",
    height: "26px",
    border: "none",
    "border-radius": "5px",
    background: "var(--btn)",
    color: "var(--tx)",
    "font-size": "13px",
    "font-weight": 600,
    padding: "3px 28px 3px 12px",
  } as const;

  const checkboxLabel = {
    display: "flex",
    "align-items": "center",
    gap: "7px",
    color: "var(--tx2)",
    "font-size": "13px",
    cursor: "pointer",
    "min-height": "18px",
  } as const;

  const actionButton = {
    border: "none",
    "border-radius": "6px",
    padding: "5px 12px",
    "font-size": "13px",
    "font-weight": 600,
    cursor: "pointer",
    height: "25px",
  } as const;

  return (
    <div
      style={backdrop}
      onClick={props.onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !busy()) submit();
          if (e.key === "Escape") props.onClose();
        }}
        style={dialog}
      >
        <div aria-hidden="true" style={icon}>
          <div style={{ position: "relative", width: "22px", height: "32px" }}>
            <span style={{ position: "absolute", left: "3px", top: "1px", width: "3px", height: "26px", background: "#fff", "border-radius": "2px" }} />
            <span style={{ position: "absolute", left: "9px", top: "1px", width: "3px", height: "26px", background: "#fff", "border-radius": "2px" }} />
            <span style={{ position: "absolute", left: "15px", top: "1px", width: "3px", height: "26px", background: "#fff", "border-radius": "2px" }} />
            <span style={{ position: "absolute", left: "4px", top: "25px", width: "14px", height: "4px", background: "#fff", "border-radius": "0 0 7px 7px" }} />
          </div>
        </div>

        <div style={{ "min-width": 0 }}>
          <div style={{ color: "var(--tx)", "font-size": "13.5px", "font-weight": 700, "line-height": 1.25 }}>
            Push
          </div>
          <div style={{ color: "var(--tx2)", "font-size": "13px", "line-height": 1.25, "margin-bottom": "24px" }}>
            Push your local changes to remote repository
          </div>

          <div style={form}>
            <label style={labelStyle}>Branch:</label>
            <select aria-label="Branch to push" value={props.state.refspec} disabled={busy()} style={selectLike}>
              <option value={props.state.refspec}>{branchLabel()}</option>
            </select>

            <label style={labelStyle}>To:</label>
            <select
              aria-label="Remote destination"
              value={remote()}
              disabled={busy()}
              onChange={(e) => setRemote(e.currentTarget.value)}
              style={selectLike}
            >
              <option value={remote()}>{destinationLabel()}</option>
            </select>

            <span />
            <label title="Bulk tag push is not available yet" style={{ ...checkboxLabel, opacity: 0.55, cursor: "not-allowed" }}>
              <input type="checkbox" disabled />
              Push all tags
            </label>

            <span />
            <label style={checkboxLabel}>
              <input
                type="checkbox"
                checked={force()}
                disabled={busy()}
                onChange={(e) => setForce(e.currentTarget.checked)}
              />
              Force push
            </label>
          </div>

          <Show when={force()}>
            <p style={{ margin: "8px 0 0 64px", color: "var(--warning)", "font-size": "12px" }}>
              Force push overwrites remote history and can lose commits. Use with care.
            </p>
          </Show>

          <div style={{ display: "flex", "justify-content": "flex-end", gap: "8px", "margin-top": "6px" }}>
            <button
              onClick={props.onClose}
              disabled={busy()}
              style={{ ...actionButton, background: "var(--btn)", color: "var(--tx2)" }}
            >
              Cancel
            </button>
            <button
              onClick={submit}
              disabled={busy() || !remote().trim()}
              style={{ ...actionButton, background: "var(--accent)", color: "var(--on-accent-strong)" }}
            >
              {busy() ? "Pushing..." : "Push"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default PushDialog;
