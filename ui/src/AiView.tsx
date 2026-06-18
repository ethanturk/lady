import { createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { RepoId } from "./commands";
import {
  type CommitPlan,
  type PlannedCommit,
  cancelAi,
  isConsentError,
  runAiStream,
} from "./ai";

/**
 * The AI workspace (PH5-007/008/010): Commit Composer (split working changes
 * into logical commits, review + apply), Explain (commit/range/stash/working),
 * and a Changelog generator. All flows are streamed and review-gated; nothing
 * is committed/applied without an explicit click.
 */
const AiView: Component<{ repoId: RepoId; onChanged?: () => void }> = (props) => {
  const [tool, setTool] = createSignal<"composer" | "explain" | "changelog">("composer");
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [reqId, setReqId] = createSignal<string | null>(null);

  const handleErr = (e: unknown) => {
    const msg = String(e);
    setErr(
      isConsentError(msg)
        ? "AI consent required — enable the provider and grant consent in Settings."
        : msg,
    );
  };

  const cancel = () => {
    const id = reqId();
    if (id) cancelAi(id).catch(() => {});
  };

  // ── Composer ──
  const [plan, setPlan] = createSignal<CommitPlan | null>(null);
  const [applied, setApplied] = createSignal<string | null>(null);

  const compose = async () => {
    setErr(null);
    setApplied(null);
    setPlan(null);
    setBusy(true);
    try {
      const p = await invoke<CommitPlan>("ai_compose_commits", {
        repo: props.repoId,
        reqId: `ai-composer-${Date.now()}`,
      });
      setPlan(p);
    } catch (e) {
      handleErr(e);
    } finally {
      setBusy(false);
    }
  };

  const editMessage = (i: number, msg: string) => {
    const p = plan();
    if (!p) return;
    const commits = p.commits.map((c, idx) => (idx === i ? { ...c, message: msg } : c));
    setPlan({ commits });
  };

  const applyPlan = async () => {
    const p = plan();
    if (!p) return;
    setBusy(true);
    setErr(null);
    try {
      const n = await invoke<number>("ai_apply_commit_plan", { repo: props.repoId, plan: p });
      setApplied(`Created ${n} commit${n === 1 ? "" : "s"}.`);
      setPlan(null);
      props.onChanged?.();
    } catch (e) {
      handleErr(e);
    } finally {
      setBusy(false);
    }
  };

  // ── Explain ──
  const [explainKind, setExplainKind] = createSignal<"working" | "commit" | "branch_range" | "stash">("working");
  const [oid, setOid] = createSignal("");
  const [base, setBase] = createSignal("");
  const [head, setHead] = createSignal("");
  const [stashIdx, setStashIdx] = createSignal(0);
  const [explainOut, setExplainOut] = createSignal("");

  const explainTarget = () => {
    switch (explainKind()) {
      case "commit":
        return { kind: "commit", oid: oid().trim() };
      case "branch_range":
        return { kind: "branch_range", base: base().trim(), head: head().trim() };
      case "stash":
        return { kind: "stash", index: stashIdx() };
      default:
        return { kind: "working" };
    }
  };

  const explain = async (regenerate: boolean) => {
    setErr(null);
    setExplainOut("");
    setBusy(true);
    try {
      const full = await runAiStream(
        "ai_explain",
        { repo: props.repoId, target: explainTarget(), regenerate },
        (acc) => setExplainOut(acc),
        setReqId,
      );
      setExplainOut(full);
    } catch (e) {
      handleErr(e);
    } finally {
      setBusy(false);
      setReqId(null);
    }
  };

  // ── Changelog ──
  const [clBase, setClBase] = createSignal("");
  const [clHead, setClHead] = createSignal("");
  const [clOut, setClOut] = createSignal("");

  const changelog = async () => {
    setErr(null);
    setClOut("");
    setBusy(true);
    try {
      const full = await runAiStream(
        "ai_changelog",
        { repo: props.repoId, base: clBase().trim(), head: clHead().trim() },
        (acc) => setClOut(acc),
        setReqId,
      );
      setClOut(full);
    } catch (e) {
      handleErr(e);
    } finally {
      setBusy(false);
      setReqId(null);
    }
  };

  const field = {
    border: "1px solid var(--border)",
    "border-radius": "4px",
    padding: "0.25rem 0.4rem",
    "font-size": "0.82rem",
    background: "var(--surface)",
    color: "var(--fg)",
  } as const;
  const out = {
    ...field,
    width: "100%",
    "box-sizing": "border-box",
    "min-height": "10rem",
    resize: "vertical",
    "font-family": "ui-monospace, monospace",
    "font-size": "0.8rem",
    padding: "0.4rem",
  } as const;
  const toolBtn = (t: "composer" | "explain" | "changelog") => ({
    padding: "0.25rem 0.7rem",
    "font-size": "0.82rem",
    border: "1px solid var(--border)",
    "border-radius": "4px",
    cursor: "pointer",
    background: tool() === t ? "var(--accent)" : "var(--surface)",
    color: tool() === t ? "var(--on-accent)" : "var(--fg)",
  });

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.9rem 1rem", "max-width": "44rem" }}>
      <div style={{ display: "flex", gap: "0.4rem", "margin-bottom": "0.8rem" }}>
        <button style={toolBtn("composer")} onClick={() => setTool("composer")}>
          Commit Composer
        </button>
        <button style={toolBtn("explain")} onClick={() => setTool("explain")}>
          Explain
        </button>
        <button style={toolBtn("changelog")} onClick={() => setTool("changelog")}>
          Changelog
        </button>
        <span style={{ flex: "1" }} />
        <Show when={busy() && reqId()}>
          <button style={field} onClick={cancel}>
            Cancel
          </button>
        </Show>
      </div>

      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.82rem" }}>{err()}</p>
      </Show>

      {/* Composer */}
      <Show when={tool() === "composer"}>
        <p style={{ "font-size": "0.82rem", color: "var(--fg-muted, #888)" }}>
          Organize your working changes into logical commits. Review and edit
          before applying — nothing is committed automatically.
        </p>
        <button style={{ ...field, cursor: "pointer", "margin-bottom": "0.6rem" }} disabled={busy()} onClick={compose}>
          {busy() ? "Thinking…" : "✨ Propose commits"}
        </button>
        <Show when={applied()}>
          <p style={{ color: "#1a7f37", "font-size": "0.82rem" }}>{applied()}</p>
        </Show>
        <Show when={plan()}>
          <For each={plan()!.commits}>
            {(c: PlannedCommit, i) => (
              <div style={{ border: "1px solid var(--border)", "border-radius": "4px", padding: "0.5rem", "margin-bottom": "0.5rem" }}>
                <input
                  style={{ ...field, width: "100%", "box-sizing": "border-box", "margin-bottom": "0.3rem" }}
                  value={c.message}
                  onInput={(e) => editMessage(i(), e.currentTarget.value)}
                />
                <div style={{ "font-size": "0.74rem", color: "var(--fg-muted, #888)" }}>
                  {c.hunk_ids.length} hunk(s): {c.hunk_ids.join(", ")}
                </div>
              </div>
            )}
          </For>
          <div style={{ display: "flex", gap: "0.4rem" }}>
            <button
              style={{ ...field, cursor: "pointer", border: "1px solid #1a7f37", color: "#1a7f37" }}
              disabled={busy()}
              onClick={applyPlan}
            >
              Apply {plan()!.commits.length} commit(s)
            </button>
            <button style={{ ...field, cursor: "pointer" }} onClick={() => setPlan(null)}>
              Discard plan
            </button>
          </div>
        </Show>
      </Show>

      {/* Explain */}
      <Show when={tool() === "explain"}>
        <div style={{ display: "flex", "align-items": "center", gap: "0.6rem", "flex-wrap": "wrap", "margin-bottom": "0.5rem" }}>
          <select style={field} value={explainKind()} onChange={(e) => setExplainKind(e.currentTarget.value as ReturnType<typeof explainKind>)}>
            <option value="working">Working changes</option>
            <option value="commit">Commit</option>
            <option value="branch_range">Branch range</option>
            <option value="stash">Stash</option>
          </select>
          <Show when={explainKind() === "commit"}>
            <input style={field} placeholder="commit oid" value={oid()} onInput={(e) => setOid(e.currentTarget.value)} />
          </Show>
          <Show when={explainKind() === "branch_range"}>
            <input style={field} placeholder="base" value={base()} onInput={(e) => setBase(e.currentTarget.value)} />
            <input style={field} placeholder="head" value={head()} onInput={(e) => setHead(e.currentTarget.value)} />
          </Show>
          <Show when={explainKind() === "stash"}>
            <input style={{ ...field, width: "4rem" }} type="number" min="0" value={stashIdx()} onInput={(e) => setStashIdx(Number(e.currentTarget.value))} />
          </Show>
          <button style={{ ...field, cursor: "pointer" }} disabled={busy()} onClick={() => explain(false)}>
            ✨ Explain
          </button>
          <button style={{ ...field, cursor: "pointer" }} disabled={busy()} onClick={() => explain(true)}>
            Regenerate
          </button>
        </div>
        <textarea style={out} readonly value={explainOut()} placeholder="Explanation appears here…" />
      </Show>

      {/* Changelog */}
      <Show when={tool() === "changelog"}>
        <div style={{ display: "flex", "align-items": "center", gap: "0.6rem", "margin-bottom": "0.5rem" }}>
          <input style={field} placeholder="base ref" value={clBase()} onInput={(e) => setClBase(e.currentTarget.value)} />
          <input style={field} placeholder="head ref" value={clHead()} onInput={(e) => setClHead(e.currentTarget.value)} />
          <button style={{ ...field, cursor: "pointer" }} disabled={busy()} onClick={changelog}>
            ✨ Generate
          </button>
        </div>
        <textarea style={out} value={clOut()} onInput={(e) => setClOut(e.currentTarget.value)} placeholder="Changelog appears here…" />
      </Show>
    </div>
  );
};

export default AiView;
