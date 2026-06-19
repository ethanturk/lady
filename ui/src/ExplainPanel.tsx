import { createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import type { RepoId } from "./commands";
import { cancelAi, isConsentError, runAiStream } from "./ai";

interface ExplainPanelProps {
  repoId: RepoId;
  /** The `ai_explain` target (commits / branch range / file path / raw diff). */
  target: Record<string, unknown>;
  /** Header title (e.g. "Explain src/foo.rs"). */
  title: string;
  /** Optional subtitle line under the title. */
  subtitle?: string;
  onClose: () => void;
}

/**
 * A modal overlay that streams an AI explanation of any `ai_explain` target
 * (a commit selection, a branch range, a file's changes, or a single hunk).
 * Read-only — it never mutates the repo. Mirrors AiView's explain output
 * (streamed textarea + Regenerate).
 */
const ExplainPanel: Component<ExplainPanelProps> = (props) => {
  const [out, setOut] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const [reqId, setReqId] = createSignal<string | null>(null);

  const run = async (regenerate: boolean) => {
    setErr(null);
    setOut("");
    setBusy(true);
    try {
      const full = await runAiStream(
        "ai_explain",
        { repo: props.repoId, target: props.target, regenerate },
        (acc) => setOut(acc),
        setReqId,
      );
      setOut(full);
    } catch (e) {
      const msg = String(e);
      setErr(isConsentError(msg) ? "AI consent required — enable it in the AI settings first." : msg);
    } finally {
      setBusy(false);
      setReqId(null);
    }
  };

  const cancel = () => {
    const id = reqId();
    if (id) cancelAi(id).catch(() => {});
  };

  onMount(() => void run(false));

  return (
    <>
      <div
        style={{ position: "fixed", inset: "0", background: "rgba(0,0,0,0.45)", "z-index": "1000" }}
        onClick={() => props.onClose()}
      />
      <div
        role="dialog"
        aria-label={props.title}
        style={{
          position: "fixed",
          top: "8vh",
          left: "50%",
          transform: "translateX(-50%)",
          width: "min(720px, 92vw)",
          "max-height": "82vh",
          display: "flex",
          "flex-direction": "column",
          background: "var(--panel)",
          border: "1px solid var(--bd)",
          "border-radius": "10px",
          "box-shadow": "0 18px 48px rgba(0,0,0,0.5)",
          "z-index": "1001",
          overflow: "hidden",
        }}
      >
        <div style={{ display: "flex", "align-items": "center", gap: "8px", padding: "12px 14px", "border-bottom": "1px solid var(--bd)" }}>
          <strong style={{ "font-size": "13.5px", color: "var(--tx)" }}>{props.title}</strong>
          <span style={{ "font-size": "11.5px", color: "var(--tx3)", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap", flex: "1" }}>
            {props.subtitle ?? ""}
          </span>
          <button onClick={() => props.onClose()} style={{ border: "none", background: "transparent", color: "var(--tx3)", cursor: "pointer", "font-size": "16px" }} aria-label="Close">
            ×
          </button>
        </div>

        <Show when={err()}>
          <p role="alert" style={{ color: "var(--error)", "font-size": "12.5px", margin: "10px 14px 0" }}>{err()}</p>
        </Show>

        <textarea
          readonly
          aria-label="AI explanation"
          aria-live="polite"
          value={out()}
          placeholder={busy() ? "Thinking…" : "Explanation appears here…"}
          style={{
            flex: "1",
            margin: "12px 14px",
            "min-height": "240px",
            resize: "none",
            border: "1px solid var(--bd)",
            "border-radius": "7px",
            background: "var(--input)",
            color: "var(--tx)",
            padding: "10px 12px",
            "font-size": "13px",
            "line-height": "1.5",
            outline: "none",
          }}
        />

        <div style={{ display: "flex", gap: "8px", padding: "0 14px 14px", "justify-content": "flex-end" }}>
          <Show when={busy()}>
            <button onClick={cancel} style={{ padding: "0.35rem 0.9rem" }}>Cancel</button>
          </Show>
          <button onClick={() => run(true)} disabled={busy()} style={{ padding: "0.35rem 0.9rem" }}>
            {busy() ? "Working…" : "Regenerate"}
          </button>
        </div>
      </div>
    </>
  );
};

export default ExplainPanel;
