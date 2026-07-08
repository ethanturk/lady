import { createEffect, createMemo, createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { CommitGraphRow, RefInfo, RepoId, WalkLogGraphResult, WalkLogQuery } from "./commands";
import { relTime } from "./time";
import { authorColor, initials } from "./avatar";
import { uiPadding } from "./prefs";
import type { SizeStep } from "./prefs";

// ── Layout constants ──────────────────────────────────────────────────────────
const BASE_ROW_H = 48;
// Row height scales with the global padding step (mirrors --pad-scale), floored
// so the node/avatar never collide.
const PAD_SCALE: Record<SizeStep, number> = { s: 0.4125, m: 1, l: 1.315625, xl: 1.63125 };
const rowHeight = () => Math.max(36, Math.round(BASE_ROW_H * PAD_SCALE[uiPadding()]));
const LANE_W = 20; // horizontal pixels per lane column
const COMMIT_R = 7; // commit circle radius in CSS pixels (design node)
const MAX_GRAPH_GUTTER_W = 260;
const BATCH = 500;
const LOAD_AHEAD_PX = 800;
const BUFFER = 5;

// Lane colors. Main lane = blue, first branch = magenta (design), then a
// categorical palette of mid-tone hues that read on both themes. The node ring
// uses the lane color; the node fill is the author's avatar color.
const LANE_COLORS = ["#5b8def", "#db61a2", "#0a8a0a", "#cc7700", "#8a5cf6", "#0a7a8a"];
const laneColor = (lane: number) => LANE_COLORS[lane % LANE_COLORS.length];

// ── Canvas draw ───────────────────────────────────────────────────────────────

function drawGraph(canvas: HTMLCanvasElement, rows: CommitGraphRow[], scrollTop: number, viewportH: number, rowH: number) {
  const dpr = window.devicePixelRatio || 1;
  const graphW = canvas.width / dpr;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  ctx.clearRect(0, 0, graphW, viewportH);
  ctx.save();
  ctx.scale(dpr, dpr);
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";

  const sr = Math.max(0, Math.floor(scrollTop / rowH) - 1);
  const er = Math.min(rows.length - 1, Math.ceil((scrollTop + viewportH) / rowH) + 1);

  for (let i = sr; i <= er; i++) {
    const row = rows[i];
    if (!row) break;
    const screenY = i * rowH - scrollTop;
    const cy = screenY + rowH / 2;

    // Edges from this row to the next.
    for (const edge of row.edges) {
      const x1 = edge.from_lane * LANE_W + LANE_W / 2;
      const y1 = cy;
      const x2 = edge.to_lane * LANE_W + LANE_W / 2;
      const y2 = screenY + rowH + rowH / 2;
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      if (Math.abs(x1 - x2) < 0.5) ctx.lineTo(x2, y2);
      else ctx.bezierCurveTo(x1, y1 + rowH * 0.5, x2, y2 - rowH * 0.5, x2, y2);
      ctx.strokeStyle = laneColor(edge.from_lane);
      ctx.lineWidth = 2;
      ctx.stroke();
    }

    // Commit node: author-colored fill, lane-colored ring, dark initials.
    const cx = row.lane * LANE_W + LANE_W / 2;
    ctx.beginPath();
    ctx.arc(cx, cy, COMMIT_R, 0, 2 * Math.PI);
    ctx.fillStyle = authorColor(row.author_name);
    ctx.fill();
    ctx.strokeStyle = laneColor(row.lane);
    ctx.lineWidth = 2.5;
    ctx.stroke();
    ctx.fillStyle = "#0c0d10";
    ctx.font = "700 7.5px ui-monospace, monospace";
    ctx.fillText(initials(row.author_name).slice(0, 2), cx, cy + 0.5);
  }

  ctx.restore();
}

// ── Ref chips ───────────────────────────────────────────────────────────────
type ChipKind = "head" | "remote" | "tag" | "branch";
function classifyRef(ref: string): { label: string; kind: ChipKind } {
  if (ref === "HEAD" || ref === "head:HEAD") return { label: "HEAD", kind: "head" };
  if (ref.startsWith("head:")) return { label: ref.slice(5).trim(), kind: "head" };
  if (ref.startsWith("tag:")) return { label: ref.slice(4).trim(), kind: "tag" };
  if (ref.includes("/")) return { label: ref, kind: "remote" };
  return { label: ref, kind: "branch" };
}
const chipStyle = (kind: ChipKind) => {
  const base = {
    "font-size": "11px",
    "font-weight": 600,
    "border-radius": "4px",
    padding: "1px 7px",
    border: "1px solid var(--bd)",
    "white-space": "nowrap" as const,
  };
  if (kind === "branch")
    return { ...base, color: "var(--chip-branch-tx)", background: "var(--chip-branch-bg)", "border-color": "var(--chip-branch-bd)" };
  if (kind === "remote") return { ...base, color: "var(--tx3)", background: "var(--hov)" };
  if (kind === "tag") return { ...base, color: "var(--warning)", background: "var(--warning-bg)", "border-color": "var(--warning-border)" };
  return { ...base, color: "var(--tx)", background: "var(--hov)" }; // head
};

function graphRefsForOid(refs: RefInfo[], oid: string, fallback: string[]): string[] {
  if (refs.length === 0) return fallback;

  const headNames = new Set(refs.filter((r) => r.kind === "Head").map((r) => r.name));
  return refs
    .filter((r) => r.target === oid)
    .flatMap((r) => {
      if (r.kind === "Tag") return [`tag:${r.name}`];
      if (r.kind === "Head") return [`head:${r.name}`];
      if (headNames.has(r.name)) return [];
      return [r.name];
    });
}

// ── Component ─────────────────────────────────────────────────────────────────

const GraphView: Component<{
  repoId: RepoId;
  /** Latest refs from App; used to refresh ref chips without reloading rows. */
  refs: RefInfo[];
  /** All selected commit oids (multi-select, controlled by the parent). */
  selected?: string[];
  /** The last-clicked oid — drives the detail pane and gets the accent bar. */
  primary?: string;
  /** Emits the new selection after a click resolves Cmd/Ctrl + Shift gestures. */
  onSelectionChange?: (oids: string[], primary: string) => void;
  /** Right-click a commit row → open the commit context menu at the cursor. */
  onCommitMenu?: (oid: string, summary: string, at: { x: number; y: number }) => void;
}> = (props) => {
  const [rows, setRows] = createSignal<CommitGraphRow[]>([]);
  // Anchor oid for Shift-range selection (the last plainly/Cmd-clicked row).
  const [anchor, setAnchor] = createSignal<string | null>(null);

  // Resolve a click into a new selection given the modifier keys, then hand it
  // up to the parent (which owns the selection state).
  const handleClick = (oid: string, mods: { meta: boolean; shift: boolean }) => {
    const cur = props.selected ?? [];
    if (mods.shift && anchor()) {
      const order = rows().map((r) => r.oid);
      const a = order.indexOf(anchor()!);
      const b = order.indexOf(oid);
      if (a !== -1 && b !== -1) {
        const [lo, hi] = a <= b ? [a, b] : [b, a];
        props.onSelectionChange?.(order.slice(lo, hi + 1), oid);
        return;
      }
    }
    if (mods.meta) {
      const next = cur.includes(oid) ? cur.filter((o) => o !== oid) : [...cur, oid];
      setAnchor(oid);
      // Keep a primary even after de-selecting the clicked row.
      props.onSelectionChange?.(next, next.includes(oid) ? oid : next[next.length - 1] ?? oid);
      return;
    }
    setAnchor(oid);
    props.onSelectionChange?.([oid], oid);
  };
  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewportH, setViewportH] = createSignal(400);
  const [loading, setLoading] = createSignal(false);
  const [hasMore, setHasMore] = createSignal(true);
  const [cursor, setCursor] = createSignal<string | undefined>(undefined);
  const [layoutState, setLayoutState] = createSignal<(string | null)[]>([]);

  let listContainer!: HTMLDivElement;
  let canvasEl!: HTMLCanvasElement;

  const totalH = () => rows().length * rowHeight();
  const maxLanes = createMemo(() => rows().reduce((m, r) => Math.max(m, r.num_lanes), 1));
  const graphW = () => Math.max(1, maxLanes()) * LANE_W + LANE_W;
  const graphGutterW = () => Math.min(graphW(), MAX_GRAPH_GUTTER_W);

  const startRow = () => Math.max(0, Math.floor(scrollTop() / rowHeight()) - BUFFER);
  const endRow = () => Math.min(rows().length, Math.ceil((scrollTop() + viewportH()) / rowHeight()) + BUFFER);
  const visibleSlice = createMemo(() => rows().slice(startRow(), endRow()));

  const resizeCanvas = () => {
    const dpr = window.devicePixelRatio || 1;
    const w = graphW();
    const h = viewportH();
    canvasEl.width = Math.round(w * dpr);
    canvasEl.height = Math.round(h * dpr);
    canvasEl.style.width = `${w}px`;
    canvasEl.style.height = `${h}px`;
  };

  // Resize the canvas bitmap ONLY when its dimensions change (graph width grows
  // as more lanes load, or the viewport height changes). Assigning canvas.width/
  // height reallocates and clears the backing bitmap, so doing it per scroll frame
  // is what made the graph janky. Resizing clears the canvas, so redraw here too.
  createEffect(() => {
    graphW();
    viewportH();
    if (!canvasEl) return;
    resizeCanvas();
    drawGraph(canvasEl, rows(), scrollTop(), viewportH(), rowHeight());
  });

  // Redraw on scroll (and as rows load) without touching the canvas size.
  createEffect(() => {
    const st = scrollTop();
    const allRows = rows();
    const rh = rowHeight();
    const vh = viewportH();
    if (!canvasEl) return;
    drawGraph(canvasEl, allRows, st, vh, rh);
  });

  const loadMore = async () => {
    if (loading() || !hasMore()) return;
    setLoading(true);
    try {
      const cur = cursor();
      const q: WalkLogQuery = { start: cur, limit: cur ? BATCH + 1 : BATCH };
      const result = await invoke<WalkLogGraphResult>("walk_log_graph", {
        repo: props.repoId,
        query: q,
        layoutState: cur ? layoutState() : null,
      });
      const fresh = cur ? result.rows.slice(1) : result.rows;
      setRows((prev) => [...prev, ...fresh]);
      setHasMore(fresh.length === BATCH);
      setLayoutState(result.layout_state);
      if (fresh.length > 0) setCursor(fresh[fresh.length - 1].oid);
    } finally {
      setLoading(false);
    }
  };

  onMount(async () => {
    const h = listContainer.clientHeight || 400;
    setViewportH(h);
    await loadMore();
  });

  const onScroll = () => {
    const st = listContainer.scrollTop;
    setScrollTop(st);
    if (st + viewportH() >= totalH() - LOAD_AHEAD_PX) loadMore();
  };

  // Scroll the primary commit into view when it's set from outside the graph
  // (e.g. clicking a sidebar branch). Re-runs as more history loads so a tip
  // deep in the log is reached; never scrolls a row that's already visible, so
  // ordinary in-graph clicks don't jump the viewport.
  createEffect(() => {
    const p = props.primary;
    const all = rows();
    if (!p || !listContainer) return;
    const idx = all.findIndex((r) => r.oid === p);
    if (idx === -1) {
      // Not loaded yet — pull the next batch; this effect retries on update.
      if (hasMore() && !loading()) void loadMore();
      return;
    }
    const top = idx * rowHeight();
    const vh = viewportH();
    const cur = listContainer.scrollTop;
    if (top < cur || top + rowHeight() > cur + vh) {
      listContainer.scrollTop = Math.max(0, top - vh / 2 + rowHeight() / 2);
    }
  });

  // Columns shrink (min-width 0) so a narrow window never clips the right side;
  // Description keeps priority, the rest compress with ellipsis.
  const colDesc = { flex: "1 1 0", "min-width": "90px", overflow: "hidden" } as const;
  const colAuthor = { flex: "0 1 156px", "min-width": "0", overflow: "hidden" } as const;
  const colCommit = { flex: "0 1 74px", "min-width": "0", overflow: "hidden" } as const;
  const colDate = { flex: "0 1 142px", "min-width": "0", overflow: "hidden", "text-align": "right" as const };

  return (
    <div style={{ display: "flex", "flex-direction": "column", height: "100%", overflow: "hidden", background: "var(--bg)" }}>
      {/* Column header (non-scrolling), aligned over the list via a graph-width spacer. */}
      <div
        style={{
          display: "flex",
          "align-items": "center",
          height: "30px",
          "flex-shrink": 0,
          "padding-right": "18px",
          "border-bottom": "1px solid var(--bd)",
          color: "var(--tx4)",
          "font-size": "10.5px",
          "text-transform": "uppercase",
          "letter-spacing": "0.05em",
        }}
      >
        <span style={{ width: `${graphGutterW() + 8}px`, "flex-shrink": 0 }} />
        <span style={colDesc}>Description</span>
        <span style={colAuthor}>Author</span>
        <span style={colCommit}>Commit</span>
        <span style={colDate}>Date</span>
      </div>

      <div style={{ display: "flex", flex: "1", "min-height": "0", overflow: "hidden" }}>
        {/* Canvas column: graph lanes and edges (redraws on scroll). */}
        <div style={{ width: `${graphGutterW()}px`, "flex-shrink": 0, overflow: "hidden" }}>
          <canvas ref={canvasEl} style={{ display: "block" }} onClick={(e) => {
            const rect = canvasEl.getBoundingClientRect();
            const y = e.clientY - rect.top + listContainer.scrollTop;
            const idx = Math.floor(y / rowHeight());
            const row = rows()[idx];
            if (row) handleClick(row.oid, { meta: e.metaKey || e.ctrlKey, shift: e.shiftKey });
          }} onContextMenu={(e) => {
            if (!props.onCommitMenu) return;
            const rect = canvasEl.getBoundingClientRect();
            const y = e.clientY - rect.top + listContainer.scrollTop;
            const idx = Math.floor(y / rowHeight());
            const row = rows()[idx];
            if (row) {
              e.preventDefault();
              props.onCommitMenu(row.oid, row.summary, { x: e.clientX, y: e.clientY });
            }
          }} />
        </div>

        {/* Commit list column: virtualized DOM rows. */}
        <div ref={listContainer} class="scroll-thin" style={{ flex: "1", "min-width": "0", "overflow-y": "auto" }} onScroll={onScroll}>
          <div style={{ height: `${totalH()}px`, position: "relative" }}>
            <div style={{ position: "absolute", top: `${startRow() * rowHeight()}px`, left: 0, right: 0 }}>
              <For each={visibleSlice()}>
                {(row) => {
                  const rowRefs = () => graphRefsForOid(props.refs, row.oid, row.refs);
                  const isHead = () => rowRefs().some((ref) => ref === "HEAD" || ref.startsWith("head:"));
                  const isSel = () => (props.selected ?? []).includes(row.oid);
                  const isPrimary = () => props.primary === row.oid;
                  return (
                    <div
                      class="hov"
                      onClick={(e) =>
                        handleClick(row.oid, {
                          meta: e.metaKey || e.ctrlKey,
                          shift: e.shiftKey,
                        })
                      }
                      onContextMenu={(e) => {
                        if (!props.onCommitMenu) return;
                        e.preventDefault();
                        props.onCommitMenu(row.oid, row.summary, { x: e.clientX, y: e.clientY });
                      }}
                      style={{
                        position: "relative",
                        height: `${rowHeight()}px`,
                        display: "flex",
                        "align-items": "center",
                        "padding-left": "8px",
                        "padding-right": "18px",
                        "box-sizing": "border-box",
                        "font-size": "13px",
                        cursor: "pointer",
                        background: isSel() ? "color-mix(in srgb, var(--accent) 15%, transparent)" : "transparent",
                        "box-shadow": isPrimary() ? "inset 2px 0 0 var(--accent)" : "none",
                      }}
                    >
                      {/* Description: ref chips + subject */}
                      <span style={{ ...colDesc, display: "flex", "align-items": "center", gap: "6px" }}>
                        <For each={rowRefs()}>
                          {(ref) => {
                            const c = classifyRef(ref);
                            return (
                              <span style={chipStyle(c.kind)}>
                                <Show when={c.kind === "head"}>
                                  <span style={{ color: "#46b06a", "margin-right": "3px" }}>✓</span>
                                </Show>
                                {c.label}
                              </span>
                            );
                          }}
                        </For>
                        <span style={{ overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap", color: "var(--tx)", "font-weight": isHead() ? 600 : 400 }}>
                          {row.summary}
                        </span>
                      </span>

                      {/* Author: avatar + name */}
                      <span style={{ ...colAuthor, display: "flex", "align-items": "center", gap: "7px", overflow: "hidden" }}>
                        <span
                          style={{
                            width: "19px",
                            height: "19px",
                            "flex-shrink": 0,
                            "border-radius": "50%",
                            background: authorColor(row.author_name),
                            color: "#0c0d10",
                            display: "inline-flex",
                            "align-items": "center",
                            "justify-content": "center",
                            "font-size": "9px",
                            "font-weight": 700,
                          }}
                        >
                          {initials(row.author_name)}
                        </span>
                        <span style={{ color: "var(--tx2)", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>{row.author_name}</span>
                      </span>

                      <span style={{ ...colCommit, "font-family": "ui-monospace, monospace", "font-size": "12px", color: "var(--tx3)", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
                        {row.oid.slice(0, 7)}
                      </span>
                      <span style={{ ...colDate, "font-size": "12px", color: "var(--tx3)", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
                        {relTime(row.time)}
                      </span>
                    </div>
                  );
                }}
              </For>
            </div>
          </div>
          <Show when={loading()}>
            <div style={{ "text-align": "center", padding: "0.4rem", color: "var(--tx3)", "font-size": "0.8rem" }}>Loading…</div>
          </Show>
        </div>
      </div>
    </div>
  );
};

export default GraphView;
