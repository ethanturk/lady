import { createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { CommitMeta, RebaseAction, RebaseOutcome, RebaseStep, RepoId } from "./commands";

/** A row in the rebase editor: a commit + its chosen action + optional message. */
interface Row {
  oid: string;
  summary: string;
  action: RebaseAction;
  message: string;
}

const ACTIONS: RebaseAction[] = ["Pick", "Reword", "Edit", "Squash", "Fixup", "Drop"];

interface RebaseRange {
  onto: string;
  commits: CommitMeta[];
}

/**
 * Interactive-rebase editor (PH3-004). Lists the commit range `from`→HEAD with a
 * per-commit action picker and drag-to-reorder; reword reveals a message box.
 * Run executes PH3-003 and reports the outcome to the parent, which hands a
 * conflict off to the 3-pane resolver.
 */
const InteractiveRebase: Component<{
  repoId: RepoId;
  fromOid: string;
  onClose: () => void;
  onComplete: (outcome: RebaseOutcome) => void;
}> = (props) => {
  const [onto, setOnto] = createSignal<string | null>(null);
  const [rows, setRows] = createSignal<Row[]>([]);
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [dragIdx, setDragIdx] = createSignal<number | null>(null);

  onMount(() => {
    invoke<RebaseRange>("rebase_range", { repo: props.repoId, from: props.fromOid })
      .then((range) => {
        setOnto(range.onto);
        setRows(
          range.commits.map((c) => ({
            oid: c.oid,
            summary: c.summary,
            action: "Pick" as RebaseAction,
            message: c.summary,
          })),
        );
      })
      .catch((e) => setErr(String(e)));
  });

  const setRow = (i: number, patch: Partial<Row>) =>
    setRows((prev) => prev.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  // HTML5 drag-to-reorder: move the dragged row to the drop target's index.
  const onDrop = (target: number) => {
    const from = dragIdx();
    setDragIdx(null);
    if (from == null || from === target) return;
    setRows((prev) => {
      const next = [...prev];
      const [moved] = next.splice(from, 1);
      next.splice(target, 0, moved);
      return next;
    });
  };

  const run = () => {
    const target = onto();
    if (target == null) return;
    setErr(null);
    setBusy(true);
    const plan: RebaseStep[] = rows().map((r) => ({
      oid: r.oid,
      action: r.action,
      message: r.action === "Reword" || r.action === "Squash" ? r.message : null,
    }));
    invoke<RebaseOutcome>("rebase_interactive", { repo: props.repoId, onto: target, plan })
      .then((outcome) => {
        setBusy(false);
        props.onComplete(outcome);
      })
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  const overlay = {
    position: "fixed" as const,
    inset: "0",
    background: "rgba(0,0,0,0.35)",
    display: "flex",
    "align-items": "center",
    "justify-content": "center",
    "z-index": "50",
  };
  const card = {
    background: "#fff",
    "border-radius": "6px",
    width: "640px",
    "max-width": "92vw",
    "max-height": "84vh",
    display: "flex",
    "flex-direction": "column" as const,
    "box-shadow": "0 8px 30px rgba(0,0,0,0.25)",
  };

  return (
    <div style={overlay} onClick={props.onClose}>
      <div style={card} onClick={(e) => e.stopPropagation()}>
        <div style={{ padding: "0.6rem 0.8rem", "border-bottom": "1px solid #eee", "font-weight": 600 }}>
          Interactive rebase — {rows().length} commit{rows().length === 1 ? "" : "s"}
          <Show when={onto()}>
            <span style={{ color: "#888", "font-weight": 400, "font-size": "0.8rem", "margin-left": "0.5rem" }}>
              onto {onto()!.slice(0, 8)}
            </span>
          </Show>
        </div>

        <Show when={err()}>
          <p style={{ color: "crimson", margin: "0.4rem 0.8rem", "font-size": "0.85rem" }}>{err()}</p>
        </Show>

        <div style={{ flex: "1", overflow: "auto", padding: "0.5rem 0.8rem" }}>
          <For each={rows()}>
            {(row, i) => (
              <div
                draggable={true}
                onDragStart={() => setDragIdx(i())}
                onDragOver={(e) => e.preventDefault()}
                onDrop={() => onDrop(i())}
                style={{
                  display: "flex",
                  "align-items": "center",
                  gap: "0.4rem",
                  padding: "0.3rem 0.2rem",
                  "border-bottom": "1px solid #f3f3f3",
                  background: dragIdx() === i() ? "#eef5ff" : "transparent",
                  opacity: row.action === "Drop" ? 0.45 : 1,
                }}
              >
                <span style={{ cursor: "grab", color: "#bbb", "user-select": "none" }} title="drag to reorder">
                  ⠿
                </span>
                <select
                  value={row.action}
                  onChange={(e) => setRow(i(), { action: e.currentTarget.value as RebaseAction })}
                  style={{ "font-size": "0.78rem" }}
                >
                  <For each={ACTIONS}>{(a) => <option value={a}>{a.toLowerCase()}</option>}</For>
                </select>
                <span style={{ "font-family": "monospace", "font-size": "0.75rem", color: "#888" }}>
                  {row.oid.slice(0, 8)}
                </span>
                <span style={{ flex: "1", "font-size": "0.82rem", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
                  {row.summary}
                </span>
              </div>
            )}
          </For>

          {/* Reword message editors for any reword rows. */}
          <For each={rows()}>
            {(row, i) => (
              <Show when={row.action === "Reword"}>
                <div style={{ margin: "0.4rem 0" }}>
                  <div style={{ "font-size": "0.72rem", color: "#888" }}>
                    reword {row.oid.slice(0, 8)}
                  </div>
                  <textarea
                    value={row.message}
                    onInput={(e) => setRow(i(), { message: e.currentTarget.value })}
                    spellcheck={false}
                    style={{
                      width: "100%",
                      "box-sizing": "border-box",
                      "font-family": "monospace",
                      "font-size": "0.78rem",
                      "min-height": "2.6rem",
                      resize: "vertical",
                    }}
                  />
                </div>
              </Show>
            )}
          </For>
        </div>

        <div style={{ padding: "0.6rem 0.8rem", "border-top": "1px solid #eee", display: "flex", gap: "0.5rem", "justify-content": "flex-end" }}>
          <button onClick={props.onClose} disabled={busy()}>
            Cancel
          </button>
          <button
            onClick={run}
            disabled={busy() || onto() == null || rows().length === 0}
            style={{ background: "#0070f3", color: "#fff", border: "none", "border-radius": "3px", padding: "0.3rem 0.8rem", cursor: "pointer" }}
          >
            {busy() ? "Running…" : "Run rebase"}
          </button>
        </div>
      </div>
    </div>
  );
};

export default InteractiveRebase;
