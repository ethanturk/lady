import { createSignal, Show } from "solid-js";
import type { Component } from "solid-js";
import type { RefInfo, RepoId, SignatureStatus } from "./commands";
import GraphView from "./GraphView";
import CommitDetail from "./CommitDetail";
import { ExplainPanel, LazyViewBoundary } from "./lazyViews";
import { commitDetailHeight, hideResizers, setCommitDetailHeight } from "./prefs";

interface AllCommitsViewProps {
  repoId: RepoId;
  refs: RefInfo[];
  /** All selected commit oids. */
  selected: string[];
  /** The primary (last-clicked) oid; drives the detail pane. */
  primary: string | undefined;
  onSelectionChange: (oids: string[], primary: string) => void;
  sig?: SignatureStatus;
  onCherryPick: () => void;
  onRevert: () => void;
  onRebaseInteractive: (oid: string) => void;
  onRecompose: (oid: string) => void;
  /** Right-click a commit row → open the commit context menu at the cursor. */
  onCommitMenu?: (oid: string, summary: string, at: { x: number; y: number }) => void;
}

/**
 * The All Commits view (design): the commit graph + history list on top, and a
 * commit detail pane (Commit / Changes / File Tree) filling the bottom 46% when
 * a commit is selected. A toolbar surfaces "Explain (N)" for the selection.
 */
const AllCommitsView: Component<AllCommitsViewProps> = (props) => {
  const [explainOpen, setExplainOpen] = createSignal(false);
  let rootEl!: HTMLDivElement;

  const startDetailDrag = (e: PointerEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = commitDetailHeight();
    const rootH = rootEl?.clientHeight ?? window.innerHeight;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const next = startH + (startY - ev.clientY);
      setCommitDetailHeight(Math.max(180, Math.min(next, rootH - 180)));
    };
    const up = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  return (
    <div ref={rootEl} style={{ height: "100%", display: "flex", "flex-direction": "column", overflow: "hidden" }}>
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: "10px",
          padding: "6px 12px",
          "border-bottom": "1px solid var(--bd)",
          "flex-shrink": 0,
        }}
      >
        <span style={{ "font-size": "12px", color: "var(--tx3)" }}>
          {props.selected.length > 0
            ? `${props.selected.length} selected`
            : "Cmd-click / Shift-click to select commits"}
        </span>
        <span style={{ flex: "1" }} />
        <button
          disabled={props.selected.length === 0}
          onClick={() => setExplainOpen(true)}
          title="Explain the selected commits with AI"
          style={{
            padding: "0.3rem 0.8rem",
            "font-size": "12px",
            opacity: props.selected.length === 0 ? 0.5 : 1,
            cursor: props.selected.length === 0 ? "default" : "pointer",
          }}
        >
          ✨ Explain{props.selected.length > 0 ? ` (${props.selected.length})` : ""}
        </button>
      </div>

      <div style={{ flex: "1", "min-height": "0" }}>
        <GraphView
          repoId={props.repoId}
          refs={props.refs}
          selected={props.selected}
          primary={props.primary}
          onSelectionChange={props.onSelectionChange}
          onCommitMenu={props.onCommitMenu}
        />
      </div>

      <Show when={props.primary}>
        <>
          <Show when={!hideResizers()}>
            <div
              onPointerDown={startDetailDrag}
              title="Drag to resize the commit details"
              style={{
                "flex-shrink": 0,
                height: "7px",
                cursor: "row-resize",
                background: "var(--sub)",
                "border-top": "1px solid var(--bd)",
                "border-bottom": "1px solid var(--bd)",
                "touch-action": "none",
              }}
            />
          </Show>
          <div style={{ height: `${commitDetailHeight()}px`, "min-height": "180px", "flex-shrink": 0 }}>
            <CommitDetail
              repoId={props.repoId}
              sha={props.primary!}
              selectedShas={props.selected}
              refs={props.refs}
              sig={props.sig}
              onCherryPick={props.onCherryPick}
              onRevert={props.onRevert}
              onRebaseInteractive={props.onRebaseInteractive}
              onRecompose={props.onRecompose}
            />
          </div>
        </>
      </Show>

      <Show when={explainOpen()}>
        <LazyViewBoundary>
          <ExplainPanel
            repoId={props.repoId}
            target={{ kind: "commits", oids: props.selected }}
            title={`Explain ${props.selected.length} commit${props.selected.length === 1 ? "" : "s"}`}
            subtitle={props.selected.map((o) => o.slice(0, 8)).join(", ")}
            onClose={() => setExplainOpen(false)}
          />
        </LazyViewBoundary>
      </Show>
    </div>
  );
};

export default AllCommitsView;
