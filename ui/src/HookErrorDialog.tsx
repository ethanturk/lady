import { onCleanup, onMount } from "solid-js";
import type { Component } from "solid-js";
import { IconAlert } from "./icons";

/**
 * Centered modal that surfaces pre-commit (and other git hook) failures.
 *
 * Git hook output is verbose and multi-line — dumping it inline into the
 * Changes column overflows the layout (it bleeds across the file list). This
 * dialog gives that output a dedicated, scrollable home. It is toggled from a
 * top-bar alert icon and auto-opens whenever a new hook error arrives; the
 * underlying error is cleared on the next successful commit.
 */
const HookErrorDialog: Component<{ text: string; onClose: () => void }> = (props) => {
  // Esc closes (hides) the dialog — the icon stays so it can be reopened.
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        props.onClose();
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
      onClick={props.onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Pre-commit hook errors"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: "640px",
          "max-width": "100%",
          "max-height": "80vh",
          display: "flex",
          "flex-direction": "column",
          background: "var(--panel)",
          border: "1px solid var(--bd)",
          "border-radius": "10px",
          "box-shadow": "0 18px 50px rgba(0,0,0,0.5)",
          overflow: "hidden",
        }}
      >
        {/* Header */}
        <div
          style={{
            display: "flex",
            "align-items": "center",
            gap: "9px",
            padding: "13px 16px",
            "border-bottom": "1px solid var(--bd)",
            background: "var(--sub)",
            "flex-shrink": 0,
          }}
        >
          <span style={{ color: "var(--error)", display: "flex" }}>
            <IconAlert size={17} />
          </span>
          <span style={{ "font-size": "13px", "font-weight": 600, color: "var(--tx)" }}>
            Pre-commit hooks failed
          </span>
          <span style={{ flex: "1" }} />
          <button
            class="hov"
            aria-label="Close"
            title="Close (Esc)"
            onClick={props.onClose}
            style={{
              display: "flex",
              "align-items": "center",
              "justify-content": "center",
              width: "22px",
              height: "22px",
              border: "none",
              background: "transparent",
              "border-radius": "5px",
              color: "var(--tx3)",
              "font-size": "16px",
              "line-height": "1",
              cursor: "pointer",
            }}
          >
            ×
          </button>
        </div>

        {/* Scrollable hook output (verbatim, monospace). */}
        <div
          class="scroll-thin"
          style={{
            flex: "1 1 auto",
            "min-height": "0",
            "overflow-y": "auto",
            padding: "14px 16px",
          }}
        >
          <pre
            style={{
              margin: 0,
              "white-space": "pre-wrap",
              "word-break": "break-word",
              "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace",
              "font-size": "12px",
              "line-height": "1.5",
              color: "var(--tx2)",
            }}
          >
            {props.text}
          </pre>
        </div>

        {/* Footer */}
        <div
          style={{
            display: "flex",
            "justify-content": "flex-end",
            gap: "8px",
            padding: "11px 16px",
            "border-top": "1px solid var(--bd)",
            "flex-shrink": 0,
          }}
        >
          <button
            onClick={props.onClose}
            style={{
              padding: "7px 16px",
              "font-size": "12.5px",
              "font-weight": 600,
              background: "var(--accent)",
              color: "var(--on-accent-strong)",
              border: "none",
              "border-radius": "7px",
              cursor: "pointer",
            }}
          >
            Dismiss
          </button>
        </div>
      </div>
    </div>
  );
};

export default HookErrorDialog;
