import { createSignal, For, onCleanup, onMount } from "solid-js";
import type { Component } from "solid-js";

/** A reconcile strategy git understands for a diverged pull. */
export type PullStrategy = "merge" | "rebase" | "ff-only";

const OPTIONS: { key: PullStrategy; label: string; desc: string }[] = [
  {
    key: "merge",
    label: "Merge",
    desc: "Join the remote work into yours with a merge commit. Safest default; keeps every commit.",
  },
  {
    key: "rebase",
    label: "Rebase",
    desc: "Replay your local commits on top of the remote for a linear history. Rewrites your unpushed commits.",
  },
  {
    key: "ff-only",
    label: "Fast-forward only",
    desc: "Only advance if there is nothing to reconcile. Aborts when the branches have truly diverged.",
  },
];

/**
 * Centered modal shown when `git pull` reports divergent branches and no
 * reconcile strategy is configured. Lets the user pick merge / rebase /
 * fast-forward-only for this pull, optionally persisting it as the repo default
 * (so they aren't re-prompted) — mirroring git's own `pull.rebase` / `pull.ff`
 * hint, but as a choice instead of a fatal error.
 */
const PullReconcileDialog: Component<{
  onChoose: (strategy: PullStrategy, remember: boolean) => void;
  onCancel: () => void;
}> = (props) => {
  const [remember, setRemember] = createSignal(false);

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        props.onCancel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    onCleanup(() => window.removeEventListener("keydown", onKey, true));
  });

  return (
    <div
      style={{
        position: "fixed",
        inset: "0",
        background: "rgba(0,0,0,0.45)",
        display: "flex",
        "align-items": "center",
        "justify-content": "center",
        "z-index": "1300",
        padding: "24px",
      }}
      onClick={props.onCancel}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Reconcile divergent branches"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: "520px",
          "max-width": "100%",
          display: "flex",
          "flex-direction": "column",
          background: "var(--panel)",
          border: "1px solid var(--bd)",
          "border-radius": "10px",
          "box-shadow": "0 18px 50px rgba(0,0,0,0.5)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            padding: "14px 16px",
            "border-bottom": "1px solid var(--bd)",
            background: "var(--sub)",
          }}
        >
          <div style={{ "font-size": "13px", "font-weight": 600, color: "var(--tx)" }}>
            Branches have diverged
          </div>
          <div style={{ "font-size": "12px", color: "var(--tx3)", "margin-top": "4px" }}>
            Your branch and the remote both have new commits. Choose how to combine them.
          </div>
        </div>

        <div style={{ display: "flex", "flex-direction": "column", padding: "8px" }}>
          <For each={OPTIONS}>
            {(opt) => (
              <button
                class="hov"
                onClick={() => props.onChoose(opt.key, remember())}
                style={{
                  display: "flex",
                  "flex-direction": "column",
                  gap: "3px",
                  "text-align": "left",
                  padding: "10px 12px",
                  border: "none",
                  background: "transparent",
                  "border-radius": "7px",
                  cursor: "pointer",
                }}
              >
                <span style={{ "font-size": "12.5px", "font-weight": 600, color: "var(--tx)" }}>
                  {opt.label}
                </span>
                <span style={{ "font-size": "11.5px", color: "var(--tx3)", "line-height": "1.45" }}>
                  {opt.desc}
                </span>
              </button>
            )}
          </For>
        </div>

        <div
          style={{
            display: "flex",
            "align-items": "center",
            gap: "10px",
            padding: "11px 16px",
            "border-top": "1px solid var(--bd)",
          }}
        >
          <label style={{ display: "flex", "align-items": "center", gap: "7px", cursor: "pointer", "font-size": "12px", color: "var(--tx2)" }}>
            <input
              type="checkbox"
              checked={remember()}
              onChange={(e) => setRemember(e.currentTarget.checked)}
            />
            Remember this choice for this repo
          </label>
          <span style={{ flex: "1" }} />
          <button
            onClick={props.onCancel}
            style={{
              padding: "7px 16px",
              "font-size": "12.5px",
              background: "var(--pill)",
              color: "var(--tx)",
              border: "1px solid var(--bd)",
              "border-radius": "7px",
              cursor: "pointer",
            }}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
};

export default PullReconcileDialog;
