import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onMount,
  Show,
} from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type {
  CommitGraphRow,
  RepoId,
  WalkLogGraphResult,
  WalkLogQuery,
} from "./commands";
import { relTime } from "./time";

// ── Layout constants ──────────────────────────────────────────────────────────
const ROW_H = 48;
const LANE_W = 16; // horizontal pixels per lane column
const COMMIT_R = 4; // commit circle radius in CSS pixels
const BATCH = 500;
const LOAD_AHEAD_PX = 800;
const BUFFER = 5;

// Lane colors (cycled for different lanes)
const LANE_COLORS = ["#0070f3", "#cc2200", "#0a8a0a", "#8a0a8a", "#cc7700", "#0a7a8a"];
const laneColor = (lane: number) => LANE_COLORS[lane % LANE_COLORS.length];

// ── Canvas draw ───────────────────────────────────────────────────────────────

function drawGraph(
  canvas: HTMLCanvasElement,
  rows: CommitGraphRow[],
  scrollTop: number,
  viewportH: number,
) {
  const dpr = window.devicePixelRatio || 1;
  const graphW = canvas.width / dpr;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  ctx.clearRect(0, 0, graphW, viewportH);
  ctx.save();
  ctx.scale(dpr, dpr);

  const sr = Math.max(0, Math.floor(scrollTop / ROW_H) - 1);
  const er = Math.min(rows.length - 1, Math.ceil((scrollTop + viewportH) / ROW_H) + 1);

  for (let i = sr; i <= er; i++) {
    const row = rows[i];
    if (!row) break;
    const screenY = i * ROW_H - scrollTop;
    const cy = screenY + ROW_H / 2;

    // Draw edges from this row to the next.
    for (const edge of row.edges) {
      const x1 = edge.from_lane * LANE_W + LANE_W / 2;
      const y1 = cy;
      const x2 = edge.to_lane * LANE_W + LANE_W / 2;
      const y2 = screenY + ROW_H + ROW_H / 2;
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      if (Math.abs(x1 - x2) < 0.5) {
        ctx.lineTo(x2, y2);
      } else {
        ctx.bezierCurveTo(x1, y1 + ROW_H * 0.5, x2, y2 - ROW_H * 0.5, x2, y2);
      }
      ctx.strokeStyle = laneColor(edge.from_lane);
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }

    // Draw commit circle on top of edges.
    const cx = row.lane * LANE_W + LANE_W / 2;
    ctx.beginPath();
    ctx.arc(cx, cy, COMMIT_R, 0, 2 * Math.PI);
    ctx.fillStyle = laneColor(row.lane);
    ctx.fill();
    ctx.strokeStyle = "#fff";
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }

  ctx.restore();
}

// ── Component ─────────────────────────────────────────────────────────────────

const GraphView: Component<{ repoId: RepoId }> = (props) => {
  const [rows, setRows] = createSignal<CommitGraphRow[]>([]);
  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewportH, setViewportH] = createSignal(400);
  const [loading, setLoading] = createSignal(false);
  const [hasMore, setHasMore] = createSignal(true);
  const [cursor, setCursor] = createSignal<string | undefined>(undefined);
  const [layoutState, setLayoutState] = createSignal<(string | null)[]>([]);

  let listContainer!: HTMLDivElement;
  let canvasEl!: HTMLCanvasElement;

  const totalH = () => rows().length * ROW_H;

  // Max lanes across loaded rows — determines canvas width.
  const maxLanes = createMemo(() =>
    rows().reduce((m, r) => Math.max(m, r.num_lanes), 1),
  );
  const graphW = () => Math.max(1, maxLanes()) * LANE_W + LANE_W;

  // Virtual window for DOM rows.
  const startRow = () => Math.max(0, Math.floor(scrollTop() / ROW_H) - BUFFER);
  const endRow = () =>
    Math.min(rows().length, Math.ceil((scrollTop() + viewportH()) / ROW_H) + BUFFER);
  const visibleSlice = createMemo(() => rows().slice(startRow(), endRow()));

  // Resize and redraw canvas whenever scroll/rows/viewport change.
  const resizeCanvas = () => {
    const dpr = window.devicePixelRatio || 1;
    const w = graphW();
    const h = viewportH();
    canvasEl.width = Math.round(w * dpr);
    canvasEl.height = Math.round(h * dpr);
    canvasEl.style.width = `${w}px`;
    canvasEl.style.height = `${h}px`;
  };

  createEffect(() => {
    // Track reactive dependencies.
    const st = scrollTop();
    const vh = viewportH();
    const allRows = rows();
    graphW(); // track lane count change
    if (!canvasEl) return;
    resizeCanvas();
    drawGraph(canvasEl, allRows, st, vh);
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

  return (
    <div style={{ display: "flex", height: "100%", overflow: "hidden" }}>
      {/* Canvas column: graph lanes and edges (redraws on scroll) */}
      <canvas
        ref={canvasEl}
        style={{
          "flex-shrink": "0",
          "pointer-events": "none",
          "align-self": "flex-start",
        }}
      />

      {/* Commit list column: virtualized DOM rows */}
      <div
        ref={listContainer}
        style={{ flex: "1", "overflow-y": "auto" }}
        onScroll={onScroll}
      >
        {/* Full-height spacer for scroll range */}
        <div style={{ height: `${totalH()}px`, position: "relative" }}>
          <div
            style={{
              position: "absolute",
              top: `${startRow() * ROW_H}px`,
              left: 0,
              right: 0,
            }}
          >
            <For each={visibleSlice()}>
              {(row) => (
                <div
                  style={{
                    height: `${ROW_H}px`,
                    display: "flex",
                    "align-items": "center",
                    gap: "0.5rem",
                    padding: "0 0.5rem",
                    "border-bottom": "1px solid #eee",
                    "box-sizing": "border-box",
                    "font-size": "0.875rem",
                  }}
                >
                  <span
                    style={{
                      "font-family": "monospace",
                      color: "#888",
                      "min-width": "6.5ch",
                    }}
                  >
                    {row.oid.slice(0, 8)}
                  </span>
                  <span
                    style={{
                      flex: "1",
                      overflow: "hidden",
                      "text-overflow": "ellipsis",
                      "white-space": "nowrap",
                    }}
                  >
                    {row.summary}
                  </span>
                  <span
                    style={{
                      color: "#555",
                      "white-space": "nowrap",
                      "max-width": "12ch",
                      overflow: "hidden",
                      "text-overflow": "ellipsis",
                    }}
                  >
                    {row.author_name}
                  </span>
                  <span
                    style={{
                      color: "#888",
                      "white-space": "nowrap",
                      "min-width": "7ch",
                      "text-align": "right",
                    }}
                  >
                    {relTime(row.time)}
                  </span>
                </div>
              )}
            </For>
          </div>
        </div>
        <Show when={loading()}>
          <div
            style={{
              "text-align": "center",
              padding: "0.4rem",
              color: "#888",
              "font-size": "0.8rem",
            }}
          >
            Loading…
          </div>
        </Show>
      </div>
    </div>
  );
};

export default GraphView;
