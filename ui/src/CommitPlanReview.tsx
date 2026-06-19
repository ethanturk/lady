import { For } from "solid-js";
import type { Component } from "solid-js";
import type { CommitPlan } from "./ai";

interface CommitPlanReviewProps {
  /** The plan to review; each commit's message is editable. */
  plan: CommitPlan;
  /** Emits the plan with an edited message (host owns the plan state). */
  onChange: (plan: CommitPlan) => void;
  disabled?: boolean;
}

/**
 * Editable review of an AI commit plan (shared by the working-changes Composer
 * and the Recompose flow): one card per proposed commit with an editable
 * message and its hunk summary. Apply/discard actions live in the host.
 */
const CommitPlanReview: Component<CommitPlanReviewProps> = (props) => {
  const editMessage = (i: number, msg: string) => {
    props.onChange({
      commits: props.plan.commits.map((c, idx) => (idx === i ? { ...c, message: msg } : c)),
    });
  };

  return (
    <For each={props.plan.commits}>
      {(c, i) => (
        <div style={{ border: "1px solid var(--bd)", "border-radius": "6px", padding: "0.5rem", "margin-bottom": "0.5rem" }}>
          <input
            style={{
              width: "100%",
              "box-sizing": "border-box",
              "margin-bottom": "0.3rem",
              padding: "0.35rem 0.5rem",
              border: "1px solid var(--bd)",
              "border-radius": "5px",
              background: "var(--input)",
              color: "var(--tx)",
              "font-size": "0.85rem",
              outline: "none",
            }}
            value={c.message}
            disabled={props.disabled}
            onInput={(e) => editMessage(i(), e.currentTarget.value)}
          />
          <div style={{ "font-size": "0.74rem", color: "var(--tx3)" }}>
            {c.hunk_ids.length} hunk(s): {c.hunk_ids.join(", ")}
          </div>
        </div>
      )}
    </For>
  );
};

export default CommitPlanReview;
