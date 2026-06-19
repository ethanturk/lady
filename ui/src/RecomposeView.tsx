import { createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { RepoId } from "./commands";
import { type CommitPlan, type RecomposePlan, isConsentError } from "./ai";
import CommitPlanReview from "./CommitPlanReview";

interface RecomposeViewProps {
  repoId: RepoId;
  /** The oldest commit of the span; recompose covers fromOid → HEAD. */
  fromOid: string;
  onClose: () => void;
  /** Called after a successful apply so the host refreshes refs/graph/status. */
  onComplete: () => void;
}

/**
 * Recompose the commits from `fromOid` up to HEAD into fewer logical commits.
 * The AI plan is produced read-only; applying it mixed-resets the span and
 * recommits per the (editable) plan, rolling back on any failure. Destructive —
 * gated behind an explicit confirm, with a force-push warning when pushed.
 */
const RecomposeView: Component<RecomposeViewProps> = (props) => {
  const [resp, setResp] = createSignal<RecomposePlan | null>(null);
  const [plan, setPlan] = createSignal<CommitPlan | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);

  const loadPlan = async () => {
    setErr(null);
    setBusy(true);
    try {
      const r = await invoke<RecomposePlan>("ai_recompose_plan", {
        repo: props.repoId,
        fromOid: props.fromOid,
        reqId: `ai-recompose-${Date.now()}`,
      });
      setResp(r);
      setPlan(r.plan);
    } catch (e) {
      const msg = String(e);
      setErr(isConsentError(msg) ? "AI consent required — enable it in the AI settings first." : msg);
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    const p = plan();
    const r = resp();
    if (!p || !r) return;
    const from = p.commits.length;
    const warn = r.pushed
      ? "\n\nThese commits are already pushed — this will require a force-push."
      : "";
    if (!confirm(`Rewrite ${r.commit_count} commit${r.commit_count === 1 ? "" : "s"} into ${from}? This changes history.${warn}`)) {
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await invoke<number>("ai_recompose_apply", {
        repo: props.repoId,
        fromOid: props.fromOid,
        plan: p,
      });
      props.onComplete();
      props.onClose();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  onMount(() => void loadPlan());

  return (
    <>
      <div
        style={{ position: "fixed", inset: "0", background: "rgba(0,0,0,0.45)", "z-index": "1000" }}
        onClick={() => props.onClose()}
      />
      <div
        role="dialog"
        aria-label="Recompose commits"
        style={{
          position: "fixed",
          top: "8vh",
          left: "50%",
          transform: "translateX(-50%)",
          width: "min(640px, 92vw)",
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
          <strong style={{ "font-size": "13.5px", color: "var(--tx)", flex: "1" }}>
            Recompose commits {props.fromOid.slice(0, 8)} → HEAD
          </strong>
          <button onClick={() => props.onClose()} style={{ border: "none", background: "transparent", color: "var(--tx3)", cursor: "pointer", "font-size": "16px" }} aria-label="Close">
            ×
          </button>
        </div>

        <div class="scroll-thin" style={{ flex: "1", "overflow-y": "auto", padding: "12px 14px" }}>
          <Show when={err()}>
            <p role="alert" style={{ color: "var(--error)", "font-size": "12.5px", margin: "0 0 10px" }}>{err()}</p>
          </Show>
          <Show when={resp()?.pushed}>
            <p style={{ "font-size": "12px", color: "var(--warning, #d08a2a)", background: "color-mix(in srgb, var(--warning, #d08a2a) 14%, transparent)", border: "1px solid var(--warning, #d08a2a)", "border-radius": "6px", padding: "7px 10px", margin: "0 0 10px" }}>
              ⚠ These commits are already pushed. Recomposing rewrites history and will require a force-push.
            </p>
          </Show>
          <Show
            when={plan()}
            fallback={
              <p style={{ "font-size": "13px", color: "var(--tx3)" }}>{busy() ? "Planning a recompose…" : "No plan."}</p>
            }
          >
            <p style={{ "font-size": "12.5px", color: "var(--tx3)", margin: "0 0 8px" }}>
              {resp()!.commit_count} commit(s) → {plan()!.commits.length}. Review and edit the messages, then apply.
            </p>
            <CommitPlanReview plan={plan()!} onChange={setPlan} disabled={busy()} />
          </Show>
        </div>

        <div style={{ display: "flex", gap: "8px", padding: "12px 14px", "border-top": "1px solid var(--bd)", "justify-content": "flex-end" }}>
          <button onClick={() => props.onClose()} disabled={busy()} style={{ padding: "0.35rem 0.9rem" }}>Cancel</button>
          <button
            onClick={apply}
            disabled={busy() || !plan()}
            style={{ padding: "0.35rem 0.9rem", border: "1px solid var(--accent)", color: "var(--accent)", background: "transparent", "border-radius": "5px", cursor: "pointer" }}
          >
            {busy() ? "Working…" : `Apply ${plan()?.commits.length ?? 0} commit(s)`}
          </button>
        </div>
      </div>
    </>
  );
};

export default RecomposeView;
