import { createEffect, createMemo, createSignal, onMount, For, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { cancelAi, isConsentError, runAiStream } from "./ai";
import { conflictCombinedHeight, hideResizers, isNarrow, setConflictCombinedHeight } from "./prefs";
import type {
  ConflictRegion,
  ConflictState,
  ParsedConflict,
  RebaseOutcome,
  RepoId,
} from "./commands";

type Side = "theirs" | "ours";

/**
 * One conflict region's working resolution: the chosen sides in the order the
 * user checked them (so "both" concatenates in click order, not a fixed
 * theirs→ours), plus the editable assembled text.
 */
interface RegionState {
  sides: Side[];
  text: string;
}

/** A flattened render row: a shared context line, or a whole conflict hunk. */
type Row =
  | { kind: "ctx"; theirsNo: number; oursNo: number; text: string }
  | { kind: "conflict"; ri: number; region: ConflictRegion; theirsStart: number; oursStart: number };

const ADD_BG = "var(--diff-add-bg)";
const DEL_BG = "var(--diff-del-bg)";
// Combined-pane line tints by origin, matching the Theirs/Ours checkbox colors.
const TINT_THEIRS = "color-mix(in srgb, var(--info) 16%, transparent)";
const TINT_OURS = "color-mix(in srgb, var(--success) 16%, transparent)";

const cell: JSX.CSSProperties = {
  "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace",
  "font-size": "0.78rem",
  "line-height": "1.55",
  "white-space": "pre",
  overflow: "hidden",
  "text-overflow": "clip",
  padding: "0 0.5rem",
};

const gutterCell: JSX.CSSProperties = {
  ...cell,
  "text-align": "right",
  color: "var(--fg-muted)",
  "user-select": "none",
  background: "var(--surface-2)",
};

/** Join the chosen lines of a region for an initial editable resolution. */
function sideLines(region: ConflictRegion, side: Side): string[] {
  return side === "theirs" ? region.theirs : region.ours;
}

/** Assemble a region's text from the chosen sides, in the given order. */
function buildSideText(region: ConflictRegion, sides: Side[]): string {
  return sides.flatMap((s) => sideLines(region, s)).join("\n");
}

/**
 * 2-pane merge conflict resolver (PH3-002). Walks the repo's conflicted files
 * one at a time. The file is rendered as a single line-numbered grid: shared
 * context lines plus, per conflict, a Theirs | Ours hunk you pick a side of via
 * checkboxes (or edit by hand). Saving writes the resolution and marks the file
 * resolved (PH3-001 commands), then advances. Completing all files calls
 * `onDone`.
 */
const ConflictResolver: Component<{
  repoId: RepoId;
  refreshNonce: number;
  conflictState: ConflictState;
  onChanged: () => void;
  onDone: () => void;
}> = (props) => {
  const [paths, setPaths] = createSignal<string[]>([]);
  const [idx, setIdx] = createSignal(0);
  const [parsed, setParsed] = createSignal<ParsedConflict | null>(null);
  const [regions, setRegions] = createSignal<RegionState[]>([]);
  const [editing, setEditing] = createSignal<number[]>([]);
  const [activeConflict, setActiveConflict] = createSignal(0);
  // Combined result: by default derived live from the per-region picks; once the
  // user edits the combined pane directly, that text takes over (override).
  const [combinedOverride, setCombinedOverride] = createSignal<string | null>(null);
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  // AI suggestion (PH5-009) — review-gated; never written without an explicit
  // Apply click.
  const [aiSuggestion, setAiSuggestion] = createSignal<string | null>(null);
  const [aiBusy, setAiBusy] = createSignal(false);
  const [aiReq, setAiReq] = createSignal<string | null>(null);

  let scrollHost: HTMLDivElement | undefined;
  let rootEl: HTMLDivElement | undefined;
  let combinedTa: HTMLTextAreaElement | undefined;
  let combinedGutter: HTMLDivElement | undefined;
  let combinedHl: HTMLDivElement | undefined;
  // Identifies the file currently parsed/rendered. Background refreshes that
  // re-emit the same path must NOT reparse (would drop edits) or re-jump.
  let loadedKey: string | null = null;

  // Resizable panes: Theirs/Ours width split (fraction for Theirs) and the
  // Combined pane height (px). hostW tracks the editor width so the drag handle
  // can sit on the column boundary.
  const [theirsRatio, setTheirsRatio] = createSignal(0.5);
  // Combined pane height: persisted px, or 1/3 of the view computed on mount
  // when unset (pref 0). Dragging persists the new height.
  const [combinedHeight, setCombinedHeightLocal] = createSignal(conflictCombinedHeight() || 176);
  const setCombinedHeight = (px: number) => {
    setCombinedHeightLocal(px);
    setConflictCombinedHeight(px);
  };
  const [hostW, setHostW] = createSignal(0);
  const gutterRem = () => (isNarrow() ? 2.6 : 3.6);
  const gutterPx = () => gutterRem() * (parseFloat(getComputedStyle(document.documentElement).fontSize) || 16);
  const syncHostW = () => setHostW(scrollHost?.clientWidth ?? 0);
  // Resting x of the column drag handle (gutter + Theirs share of the text area).
  const handleLeft = () => {
    const w = hostW();
    if (!w) return 0;
    const g = gutterPx();
    return g + theirsRatio() * Math.max(1, w - 2 * g);
  };
  onMount(() => {
    syncHostW();
    window.addEventListener("resize", syncHostW);
    // Default the Combined pane to 1/3 of the view if the user hasn't set it.
    if (!conflictCombinedHeight()) {
      const h = rootEl?.clientHeight ?? 0;
      if (h > 0) setCombinedHeightLocal(Math.round(h / 3));
    }
  });

  const startColDrag = (e: PointerEvent) => {
    e.preventDefault();
    const host = scrollHost;
    if (!host) return;
    const rect = host.getBoundingClientRect();
    setHostW(rect.width);
    const g = gutterPx();
    const tt = Math.max(1, rect.width - 2 * g);
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const r = (ev.clientX - rect.left - g) / tt;
      setTheirsRatio(Math.max(0.15, Math.min(0.85, r)));
    };
    const up = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  const startHeightDrag = (e: PointerEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = combinedHeight();
    const max = (rootEl?.clientHeight ?? window.innerHeight) - 200;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      // Drag up grows Combined / shrinks the editor; clamp both.
      const next = startH + (startY - ev.clientY);
      setCombinedHeight(Math.max(72, Math.min(next, Math.max(120, max))));
    };
    const up = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };
  // Scrollbar minimap: one mark per conflict at its fractional offset.
  const [markers, setMarkers] = createSignal<{ ri: number; pct: number }[]>([]);
  const measureMarkers = () => {
    const host = scrollHost;
    if (!host) {
      setMarkers([]);
      return;
    }
    const total = host.scrollHeight || 1;
    const out: { ri: number; pct: number }[] = [];
    host.querySelectorAll<HTMLElement>("[data-conflict]").forEach((el) => {
      out.push({ ri: Number(el.dataset.conflict), pct: (el.offsetTop / total) * 100 });
    });
    setMarkers(out);
  };

  const autoResolveAi = async () => {
    const path = current();
    if (!path || aiBusy()) return;
    setErr(null);
    setAiBusy(true);
    setAiSuggestion("");
    try {
      const full = await runAiStream(
        "ai_resolve_conflict",
        { repo: props.repoId, path },
        (acc) => setAiSuggestion(acc),
        (id) => setAiReq(id),
      );
      setAiSuggestion(full);
    } catch (e) {
      const msg = String(e);
      setErr(
        isConsentError(msg)
          ? "AI consent required — enable the provider and grant consent in Settings."
          : msg,
      );
      setAiSuggestion(null);
    } finally {
      setAiBusy(false);
      setAiReq(null);
    }
  };

  const applyAiSuggestion = () => {
    const path = current();
    const content = aiSuggestion();
    if (!path || content == null) return;
    setErr(null);
    setBusy(true);
    invoke("write_resolution", { repo: props.repoId, path, content })
      .then(() => invoke("mark_resolved", { repo: props.repoId, path }))
      .then(() => setAiSuggestion(null))
      .then(advance)
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  const current = () => paths()[idx()];

  // Reload the conflict list whenever the repo or refresh nonce changes.
  createEffect(() => {
    props.refreshNonce;
    const repo = props.repoId;
    invoke<string[]>("list_conflicts", { repo })
      .then((p) => {
        setPaths(p);
        if (idx() >= p.length) setIdx(0);
      })
      .catch((e) => setErr(String(e)));
  });

  // Load the current file's parsed regions. Only (re)parses + jumps when the
  // file actually changes; a background refresh re-emitting the same path is a
  // no-op so the user's picks, manual edits, and scroll position survive.
  createEffect(() => {
    const path = current();
    const repo = props.repoId;
    if (!path) {
      setParsed(null);
      loadedKey = null;
      return;
    }
    const key = `${JSON.stringify(repo)}\u0000${path}`;
    if (key === loadedKey) return;
    setErr(null);
    invoke<ParsedConflict>("parse_conflict", { repo, path })
      .then((p) => {
        loadedKey = key;
        setEditing([]);
        setActiveConflict(0);
        setCombinedOverride(null);
        setParsed(p);
        // One RegionState per conflict segment — nothing chosen yet, so the
        // combined result starts empty until the user picks a side.
        const rs: RegionState[] = [];
        for (const seg of p.segments) {
          if (seg.kind === "Conflict") rs.push({ sides: [], text: "" });
        }
        setRegions(rs);
        // After the grid renders, mark the scrollbar and jump to conflict #1.
        requestAnimationFrame(() => {
          measureMarkers();
          syncHostW();
          gotoConflict(0, false);
        });
      })
      .catch((e) => setErr(String(e)));
  });

  // Re-measure scrollbar marks when layout-affecting state changes.
  createEffect(() => {
    parsed();
    editing();
    isNarrow();
    requestAnimationFrame(measureMarkers);
  });

  // Flatten segments into line-numbered rows (memoized — only rebuilds when the
  // parse changes, so toggling a side just recolors existing cells).
  const rows = createMemo<Row[]>(() => {
    const out: Row[] = [];
    const p = parsed();
    if (!p) return out;
    let theirsNo = 1;
    let oursNo = 1;
    let ri = 0;
    for (const seg of p.segments) {
      if (seg.kind === "Context") {
        for (const l of seg.value) out.push({ kind: "ctx", theirsNo: theirsNo++, oursNo: oursNo++, text: l });
      } else {
        const region = seg.value;
        out.push({ kind: "conflict", ri, region, theirsStart: theirsNo, oursStart: oursNo });
        theirsNo += region.theirs.length;
        oursNo += region.ours.length;
        ri++;
      }
    }
    return out;
  });

  const conflictCount = () => regions().length;

  const setRegion = (i: number, patch: Partial<RegionState>) => {
    // A pick/edit re-derives the combined pane (discards any manual override).
    setCombinedOverride(null);
    setRegions((prev) => prev.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  };

  const theirsChecked = (ri: number) => regions()[ri]?.sides.includes("theirs") ?? false;
  const oursChecked = (ri: number) => regions()[ri]?.sides.includes("ours") ?? false;

  // Toggle a side's checkbox. Checking appends (preserving click order so "both"
  // concatenates in the order picked); unchecking removes. Text is rebuilt from
  // the resulting ordered sides.
  const toggleSide = (ri: number, side: Side, region: ConflictRegion) => {
    const prev = regions()[ri]?.sides ?? [];
    const sides = prev.includes(side) ? prev.filter((s) => s !== side) : [...prev, side];
    setRegion(ri, { sides, text: buildSideText(region, sides) });
    setActiveConflict(ri);
    scrollCombinedToRegion(ri);
  };

  const isEditing = (ri: number) => editing().includes(ri);
  const toggleEdit = (ri: number) =>
    setEditing((e) => (e.includes(ri) ? e.filter((x) => x !== ri) : [...e, ri]));

  const gotoConflict = (n: number, smooth = true) => {
    const total = conflictCount();
    if (!total) return;
    const t = ((n % total) + total) % total;
    setActiveConflict(t);
    scrollHost
      ?.querySelector(`[data-conflict="${t}"]`)
      ?.scrollIntoView({ block: "center", behavior: smooth ? "smooth" : "auto" });
  };

  const allChosen = () => regions().every((r) => r.sides.length > 0);

  /** Assemble the resolved file from context lines + per-region edited text. */
  const buildResolution = (): string => {
    const lines: string[] = [];
    let r = 0;
    for (const seg of parsed()?.segments ?? []) {
      if (seg.kind === "Context") {
        for (const l of seg.value) lines.push(l);
      } else {
        const text = regions()[r]?.text ?? "";
        if (text.length > 0) for (const l of text.split("\n")) lines.push(l);
        r++;
      }
    }
    return lines.length ? lines.join("\n") + "\n" : "";
  };

  // Live combined output (memoized) and the effective text to save: a manual
  // override if present, else the picks-derived assembly.
  const combinedAuto = createMemo(buildResolution);
  const combined = () => combinedOverride() ?? combinedAuto();
  // Per-line origin model for the combined pane's color overlay. A region whose
  // text is unedited maps to its chosen sides (in order); a hand-edited region
  // (text diverged from the assembled sides) is shown neutral since per-line
  // origin is no longer known. When the whole pane is overridden, all neutral.
  type CLine = { text: string; kind: "ctx" | Side };
  const combinedModel = createMemo<CLine[]>(() => {
    const out: CLine[] = [];
    if (combinedOverride() !== null) {
      for (const l of combinedOverride()!.replace(/\n$/, "").split("\n")) out.push({ text: l, kind: "ctx" });
      return out;
    }
    let r = 0;
    for (const seg of parsed()?.segments ?? []) {
      if (seg.kind === "Context") {
        for (const l of seg.value) out.push({ text: l, kind: "ctx" });
      } else {
        const st = regions()[r];
        if (st) {
          if (st.text === buildSideText(seg.value, st.sides)) {
            for (const side of st.sides) for (const l of sideLines(seg.value, side)) out.push({ text: l, kind: side });
          } else if (st.text.length > 0) {
            for (const l of st.text.split("\n")) out.push({ text: l, kind: "ctx" });
          }
        }
        r++;
      }
    }
    return out;
  });
  // Line-number gutter for the combined pane.
  const combinedLineNos = createMemo(() => {
    const n = combined().split("\n").length;
    let s = "";
    for (let i = 1; i <= n; i++) s += (i > 1 ? "\n" : "") + i;
    return s;
  });
  // Keep the gutter and color overlay aligned with the textarea on scroll.
  const syncCombinedGutter = () => {
    const ta = combinedTa;
    if (!ta) return;
    if (combinedGutter) combinedGutter.scrollTop = ta.scrollTop;
    if (combinedHl) {
      combinedHl.scrollTop = ta.scrollTop;
      combinedHl.scrollLeft = ta.scrollLeft;
    }
  };
  // The 0-based line in the combined output where conflict `ri` begins (mirrors
  // buildResolution's assembly — empty picks contribute no lines).
  const combinedLineStart = (ri: number) => {
    let line = 0;
    let r = 0;
    for (const seg of parsed()?.segments ?? []) {
      if (seg.kind === "Context") {
        line += seg.value.length;
      } else {
        if (r === ri) return line;
        const text = regions()[r]?.text ?? "";
        if (text.length > 0) line += text.split("\n").length;
        r++;
      }
    }
    return line;
  };
  // After a pick changes, scroll the combined pane to that conflict's section.
  const scrollCombinedToRegion = (ri: number) => {
    requestAnimationFrame(() => {
      const ta = combinedTa;
      const hl = combinedHl;
      if (!ta || !hl) return;
      const start = combinedLineStart(ri);
      // Use the overlay's real per-line layout (exact px) rather than parsing
      // line-height, which WKWebView reports unitless for `line-height: 1.5`.
      const child = hl.children[start] as HTMLElement | undefined;
      if (!child) return;
      const lineH = child.offsetHeight || 18;
      // offsetTop is relative to the overlay's padding box, matching the
      // textarea's; leave one line of context above the conflict.
      ta.scrollTop = Math.max(0, child.offsetTop - lineH);
      syncCombinedGutter();
    });
  };
  // True if the combined text still has git conflict markers (only possible via
  // a manual override that pasted/kept them).
  const hasConflictMarkers = () => /^(<{7}|={7}|>{7})/m.test(combined());
  // Save is blocked while any conflict is unresolved: every region must have a
  // side picked (or a marker-free manual override).
  const canSave = () =>
    !hasConflictMarkers() && (combinedOverride() !== null ? true : allChosen());

  const advance = () => {
    setBusy(true);
    props.onChanged();
    // After onChanged the parent bumps the nonce; reload the list to see if
    // anything remains, then either move on or finish.
    invoke<string[]>("list_conflicts", { repo: props.repoId })
      .then((p) => {
        setPaths(p);
        if (p.length === 0) {
          props.onDone();
        } else if (idx() >= p.length) {
          setIdx(0);
        }
      })
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  };

  const saveResolution = () => {
    const path = current();
    if (!path) return;
    setErr(null);
    setBusy(true);
    const content = combined();
    invoke("write_resolution", { repo: props.repoId, path, content })
      .then(() => invoke("mark_resolved", { repo: props.repoId, path }))
      .then(advance)
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  // Whole-file shortcuts via the engine (PH3-001 take_ours / take_theirs).
  const takeWholeFile = (cmd: "take_ours" | "take_theirs") => {
    const path = current();
    if (!path) return;
    setErr(null);
    setBusy(true);
    invoke(cmd, { repo: props.repoId, path })
      .then(() => invoke("mark_resolved", { repo: props.repoId, path }))
      .then(advance)
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  // Rebase sequencing controls (PH3-004 handoff). Continue/skip drive the
  // engine; the parent is told to finish when the rebase completes.
  const runRebaseStep = (cmd: "rebase_continue" | "rebase_skip") => {
    setErr(null);
    setBusy(true);
    invoke<RebaseOutcome>(cmd, { repo: props.repoId })
      .then((outcome) => {
        setBusy(false);
        if (outcome.kind === "Rebased") {
          props.onDone();
        } else {
          // More conflicts (or an edit stop) — reload and keep resolving.
          props.onChanged();
        }
      })
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  const abort = () => {
    setErr(null);
    setBusy(true);
    invoke("conflict_abort", { repo: props.repoId })
      .then(() => {
        setBusy(false);
        props.onDone();
      })
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  // Human label for the in-progress operation (Merge/Rebase/CherryPick/Revert).
  const opLabel = () => {
    switch (props.conflictState) {
      case "Rebase":
        return "rebase";
      case "CherryPick":
        return "cherry-pick";
      case "Revert":
        return "revert";
      default:
        return "merge";
    }
  };
  // Abandon the whole operation (git --abort), restoring the pre-op state.
  const abandonOp = () => {
    if (busy()) return;
    if (!confirm(`Abandon the ${opLabel()} and discard all in-progress conflict resolution? This restores the state from before the ${opLabel()}.`)) return;
    abort();
  };

  const headerBtn: JSX.CSSProperties = {
    border: "1px solid var(--border)",
    background: "var(--surface)",
    "border-radius": "3px",
    "font-size": "0.75rem",
    cursor: "pointer",
    padding: "0.2rem 0.5rem",
  };

  const navBtn: JSX.CSSProperties = {
    ...headerBtn,
    padding: "0.2rem 0.45rem",
    "line-height": 1,
    "font-weight": 700,
  };

  const gridCols = () => {
    const g = `${gutterRem()}rem`;
    return `${g} minmax(0,${theirsRatio()}fr) ${g} minmax(0,${1 - theirsRatio()}fr)`;
  };

  /** A small inline checkbox + label used in a conflict's hunk header. */
  const sideCheckbox = (label: string, checked: () => boolean, color: string, onClick: () => void) => (
    <button
      onClick={onClick}
      style={{
        display: "flex",
        "align-items": "center",
        gap: "0.3rem",
        border: "none",
        background: "transparent",
        cursor: "pointer",
        "font-size": "0.72rem",
        "font-weight": 600,
        color: "var(--fg)",
        padding: "0",
      }}
    >
      <span
        style={{
          width: "0.9rem",
          height: "0.9rem",
          "border-radius": "3px",
          border: `1px solid ${checked() ? color : "var(--border)"}`,
          background: checked() ? color : "transparent",
          color: "var(--on-accent)",
          display: "inline-flex",
          "align-items": "center",
          "justify-content": "center",
          "font-size": "0.65rem",
          "line-height": 1,
        }}
      >
        {checked() ? "✓" : ""}
      </span>
      {label}
    </button>
  );

  /** Render one conflict hunk as grid rows (header span + per-line cells). */
  const conflictRows = (row: Extract<Row, { kind: "conflict" }>) => {
    const { ri, region } = row;
    const maxLen = Math.max(region.theirs.length, region.ours.length);
    const lineIdx = Array.from({ length: maxLen }, (_, i) => i);
    return (
      <>
        {/* Hunk header: spans both panes; pick a side or edit by hand. */}
        <div
          data-conflict={ri}
          style={{
            "grid-column": "1 / -1",
            display: "flex",
            "align-items": "center",
            gap: "0.75rem",
            padding: "0.25rem 0.6rem",
            background: "var(--surface-2)",
            "border-top": "1px solid var(--warning-border)",
            "border-bottom": "1px solid var(--warning-border)",
            outline: activeConflict() === ri ? "2px solid var(--accent)" : "none",
            "outline-offset": "-2px",
          }}
        >
          <span style={{ "font-size": "0.7rem", color: "var(--fg-muted)", "font-weight": 700 }}>
            Conflict #{ri + 1}
          </span>
          {sideCheckbox("Theirs", () => theirsChecked(ri), "var(--info)", () => toggleSide(ri, "theirs", region))}
          {sideCheckbox("Ours", () => oursChecked(ri), "var(--success)", () => toggleSide(ri, "ours", region))}
          <span style={{ flex: "1" }} />
          <button
            onClick={() => toggleEdit(ri)}
            style={{
              ...headerBtn,
              "font-size": "0.7rem",
              padding: "0.1rem 0.4rem",
              color: isEditing(ri) ? "var(--accent)" : "var(--fg-muted)",
              "border-color": isEditing(ri) ? "var(--accent)" : "var(--border)",
            }}
          >
            {isEditing(ri) ? "Done editing" : "Edit"}
          </button>
        </div>

        <Show
          when={!isEditing(ri)}
          fallback={
            <textarea
              value={regions()[ri]?.text ?? ""}
              onInput={(e) => setRegion(ri, { text: e.currentTarget.value })}
              spellcheck={false}
              style={{
                "grid-column": "1 / -1",
                width: "100%",
                "box-sizing": "border-box",
                "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace",
                "font-size": "0.78rem",
                "line-height": "1.55",
                border: "none",
                "border-bottom": "1px solid var(--warning-border)",
                background: "var(--surface)",
                color: "var(--fg)",
                padding: "0.4rem 0.6rem",
                "min-height": "4rem",
                resize: "vertical",
              }}
            />
          }
        >
          <For each={lineIdx}>
            {(i) => (
              <>
                <div style={{ ...gutterCell, background: theirsChecked(ri) ? ADD_BG : DEL_BG }}>
                  {i < region.theirs.length ? row.theirsStart + i : ""}
                </div>
                <div style={{ ...cell, background: theirsChecked(ri) ? ADD_BG : DEL_BG }}>
                  {region.theirs[i] ?? ""}
                </div>
                <div style={{ ...gutterCell, background: oursChecked(ri) ? ADD_BG : DEL_BG }}>
                  {i < region.ours.length ? row.oursStart + i : ""}
                </div>
                <div style={{ ...cell, background: oursChecked(ri) ? ADD_BG : DEL_BG }}>
                  {region.ours[i] ?? ""}
                </div>
              </>
            )}
          </For>
        </Show>
      </>
    );
  };

  return (
    <div ref={rootEl} style={{ height: "100%", display: "flex", "flex-direction": "column" }}>
      <Show
        when={paths().length > 0}
        fallback={
          <p style={{ color: "var(--success)", padding: "1rem", "font-size": "0.9rem" }}>
            <Show
              when={props.conflictState === "Rebase"}
              fallback="No conflicts to resolve. 🎉"
            >
              All files resolved — continue the rebase to finish.
            </Show>
          </p>
        }
      >
        {/* Header: progress + file + conflict nav + whole-file actions */}
        <div
          style={{
            display: "flex",
            "align-items": "center",
            gap: "0.5rem",
            padding: "0.4rem 0.6rem",
            "border-bottom": "1px solid var(--border)",
            "flex-shrink": 0,
            "flex-wrap": isNarrow() ? "wrap" : "nowrap",
          }}
        >
          <span style={{ "font-size": "0.8rem", color: "var(--fg-muted)" }}>
            Conflict {idx() + 1} / {paths().length}
          </span>
          <span style={{ "font-family": "monospace", "font-size": "0.8rem", "font-weight": 600 }}>
            {current()}
          </span>
          <Show when={conflictCount() > 0}>
            <div style={{ display: "flex", "align-items": "center", gap: "0.25rem" }}>
              <button style={navBtn} disabled={busy()} title="Previous conflict" onClick={() => gotoConflict(activeConflict() - 1)}>
                ↑
              </button>
              <span style={{ "font-size": "0.72rem", color: "var(--fg-muted)", "min-width": "2.5rem", "text-align": "center" }}>
                {activeConflict() + 1}/{conflictCount()}
              </span>
              <button style={navBtn} disabled={busy()} title="Next conflict" onClick={() => gotoConflict(activeConflict() + 1)}>
                ↓
              </button>
            </div>
          </Show>
          <span style={{ flex: "1" }} />
          <button style={headerBtn} disabled={busy()} onClick={() => takeWholeFile("take_ours")}>
            Use ours
          </button>
          <button style={headerBtn} disabled={busy()} onClick={() => takeWholeFile("take_theirs")}>
            Use theirs
          </button>
          <button
            style={headerBtn}
            disabled={busy()}
            title="Open in external merge tool"
            onClick={() => {
              const path = current();
              if (!path) return;
              setErr(null);
              loadedKey = null; // force a reparse to pick up the tool's edits
              invoke("launch_mergetool", { repo: props.repoId, path })
                .then(() => props.onChanged())
                .catch((e) => setErr(String(e)));
            }}
          >
            Ext merge
          </button>
          <button
            style={{ ...headerBtn, border: "1px solid var(--accent)", color: "var(--accent)" }}
            disabled={busy() || aiBusy()}
            title="Suggest a resolution with AI (you review before applying)"
            onClick={autoResolveAi}
          >
            {aiBusy() ? "Resolving…" : "✨ Auto-resolve with AI"}
          </button>
          <button
            style={{ ...headerBtn, background: canSave() ? "var(--success)" : "var(--border)", color: canSave() ? "var(--on-accent)" : "var(--fg-muted)" }}
            disabled={busy() || !canSave()}
            title={canSave() ? "Write this file's resolution and continue" : "Resolve every conflict in this file first"}
            onClick={saveResolution}
          >
            Save &amp; next
          </button>
          <Show when={props.conflictState !== "None"}>
            <button
              style={{ ...headerBtn, color: "var(--danger)", "border-color": "var(--danger)", "margin-left": "0.4rem" }}
              disabled={busy()}
              title={`Abort the ${opLabel()} and discard all conflict resolution`}
              onClick={abandonOp}
            >
              Abandon {opLabel()}
            </button>
          </Show>
        </div>

        {/* AI suggestion panel (review-gated) — edit then Apply, or Dismiss. */}
        <Show when={aiSuggestion() !== null}>
          <div style={{ margin: "0.4rem 0.6rem", border: "1px solid var(--accent)", "border-radius": "4px", padding: "0.4rem" }}>
            <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "margin-bottom": "0.3rem" }}>
              <span style={{ "font-size": "0.78rem", "font-weight": 600, color: "var(--accent)" }}>
                AI suggestion for {current()} — review before applying
              </span>
              <span style={{ flex: "1" }} />
              <Show when={aiBusy() && aiReq()}>
                <button style={headerBtn} onClick={() => { const id = aiReq(); if (id) cancelAi(id).catch(() => {}); }}>
                  Cancel
                </button>
              </Show>
              <button style={{ ...headerBtn, border: "1px solid var(--success)", color: "var(--success)" }} disabled={busy() || aiBusy()} onClick={applyAiSuggestion}>
                Apply &amp; next
              </button>
              <button style={headerBtn} onClick={() => setAiSuggestion(null)}>
                Dismiss
              </button>
            </div>
            <textarea
              style={{ width: "100%", "box-sizing": "border-box", "min-height": "8rem", resize: "vertical", "font-family": "ui-monospace, monospace", "font-size": "0.8rem", padding: "0.35rem", border: "1px solid var(--border)", "border-radius": "4px", background: "var(--surface)", color: "var(--fg)" }}
              value={aiSuggestion() ?? ""}
              onInput={(e) => setAiSuggestion(e.currentTarget.value)}
            />
          </div>
        </Show>

        <Show when={err()}>
          <p style={{ color: "var(--error)", margin: "0.25rem 0.6rem", "font-size": "0.85rem" }}>{err()}</p>
        </Show>

        {/* Unified 2-pane editor: line-numbered Theirs | Ours, one scroll
            container so the columns stay aligned and scroll together. */}
        <div style={{ flex: "1", "min-height": "0", position: "relative", display: "flex" }}>
        <div ref={scrollHost} style={{ flex: "1", "min-height": "0", overflow: "auto", position: "relative" }}>
          {/* Sticky column headers. */}
          <div
            style={{
              position: "sticky",
              top: "0",
              "z-index": 2,
              display: "grid",
              "grid-template-columns": gridCols(),
              background: "var(--surface)",
              "border-bottom": "1px solid var(--border)",
            }}
          >
            <div style={gutterCell} />
            <div style={{ ...cell, color: "var(--info)", "font-weight": 700, padding: "0.3rem 0.5rem" }}>Theirs</div>
            <div style={gutterCell} />
            <div style={{ ...cell, color: "var(--success)", "font-weight": 700, padding: "0.3rem 0.5rem" }}>Ours</div>
          </div>

          <div style={{ display: "grid", "grid-template-columns": gridCols() }}>
            <For each={rows()}>
              {(row) =>
                row.kind === "ctx" ? (
                  <>
                    <div style={gutterCell}>{row.theirsNo}</div>
                    <div style={cell}>{row.text}</div>
                    <div style={gutterCell}>{row.oursNo}</div>
                    <div style={cell}>{row.text}</div>
                  </>
                ) : (
                  conflictRows(row)
                )
              }
            </For>
          </div>
        </div>

        {/* Scrollbar conflict markers (red = unresolved hunk position). */}
        <Show when={markers().length > 0}>
          <div style={{ position: "absolute", top: "0", right: "0", bottom: "0", width: "10px", "pointer-events": "none" }}>
            <For each={markers()}>
              {(m) => (
                <div
                  title={`Conflict #${m.ri + 1}`}
                  onClick={() => gotoConflict(m.ri)}
                  style={{
                    position: "absolute",
                    top: `${m.pct}%`,
                    right: "1px",
                    width: "8px",
                    height: "5px",
                    "border-radius": "2px",
                    background: activeConflict() === m.ri ? "var(--accent)" : "var(--danger)",
                    "pointer-events": "auto",
                    cursor: "pointer",
                  }}
                />
              )}
            </For>
          </div>
        </Show>

        {/* Drag handle: resize the Theirs / Ours panes (col-resize). */}
        <Show when={!hideResizers()}>
          <div
            onPointerDown={startColDrag}
            title="Drag to resize the Theirs / Ours panes"
            style={{
              position: "absolute",
              top: "0",
              bottom: "0",
              left: `${handleLeft()}px`,
              width: "7px",
              "margin-left": "-3px",
              cursor: "col-resize",
              "z-index": 3,
              "touch-action": "none",
            }}
          />
        </Show>
        </div>

        {/* Combined result — the file as it will be written. Live-derived from
            the picks above; edit here for a final manual merge. Height is
            drag-resizable via the handle on its top border. */}
        <div
          style={{
            "flex-shrink": 0,
            height: `${combinedHeight()}px`,
            display: "flex",
            "flex-direction": "column",
            "border-top": "1px solid var(--border)",
            background: "var(--surface-2)",
            position: "relative",
          }}
        >
          {/* Drag handle: resize the Combined pane (row-resize). */}
          <Show when={!hideResizers()}>
            <div
              onPointerDown={startHeightDrag}
              title="Drag to resize the Combined pane"
              style={{
                position: "absolute",
                top: "0",
                left: "0",
                right: "0",
                height: "7px",
                "margin-top": "-3px",
                cursor: "row-resize",
                "z-index": 3,
                "touch-action": "none",
              }}
            />
          </Show>
          <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", padding: "0.3rem 0.6rem" }}>
            <span style={{ "font-size": "0.72rem", "font-weight": 700, "letter-spacing": "0.04em", color: "var(--fg-muted)" }}>
              COMBINED RESULT
            </span>
            <Show when={combinedOverride() !== null}>
              <span style={{ "font-size": "0.7rem", color: "var(--accent)" }}>· edited</span>
            </Show>
            <span style={{ flex: "1" }} />
            <Show when={combinedOverride() !== null}>
              <button
                style={{ ...headerBtn, "font-size": "0.7rem", padding: "0.1rem 0.4rem" }}
                title="Discard manual edits and rebuild from the picks above"
                onClick={() => setCombinedOverride(null)}
              >
                ↻ Rebuild from picks
              </button>
            </Show>
          </div>
          <div style={{ flex: "1", "min-height": "0", display: "flex", "border-top": "1px solid var(--border)" }}>
            {/* Line-number gutter, scroll-synced to the textarea. */}
            <div
              ref={combinedGutter}
              aria-hidden="true"
              style={{
                "flex-shrink": 0,
                overflow: "hidden",
                "text-align": "right",
                "user-select": "none",
                "white-space": "pre",
                "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace",
                "font-size": "0.78rem",
                "line-height": "1.5",
                color: "var(--fg-muted)",
                background: "var(--surface-2)",
                "border-right": "1px solid var(--border)",
                padding: "0.4rem 0.4rem",
              }}
            >
              {combinedLineNos()}
            </div>
            {/* Editor area: color overlay behind a transparent textarea so the
                per-origin tints show through under the editable text. */}
            <div style={{ position: "relative", flex: "1", "min-width": "0", background: "var(--surface)" }}>
              <div
                ref={combinedHl}
                aria-hidden="true"
                style={{
                  position: "absolute",
                  inset: "0",
                  overflow: "hidden",
                  "pointer-events": "none",
                  "white-space": "pre",
                  "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace",
                  "font-size": "0.78rem",
                  "line-height": "1.5",
                  padding: "0.4rem 0.6rem",
                  color: "transparent",
                }}
              >
                <For each={combinedModel()}>
                  {(l) => (
                    <div
                      style={{
                        background: l.kind === "theirs" ? TINT_THEIRS : l.kind === "ours" ? TINT_OURS : "transparent",
                        "min-width": "100%",
                        width: "max-content",
                      }}
                    >
                      {l.text === "" ? " " : l.text}
                    </div>
                  )}
                </For>
              </div>
              <textarea
                ref={combinedTa}
                value={combined()}
                onInput={(e) => setCombinedOverride(e.currentTarget.value)}
                onScroll={syncCombinedGutter}
                spellcheck={false}
                style={{
                  position: "absolute",
                  inset: "0",
                  width: "100%",
                  height: "100%",
                  "box-sizing": "border-box",
                  resize: "none",
                  "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace",
                  "font-size": "0.78rem",
                  "line-height": "1.5",
                  border: "none",
                  background: "transparent",
                  color: "var(--fg)",
                  padding: "0.4rem 0.6rem",
                  "white-space": "pre",
                  "overflow-wrap": "normal",
                }}
              />
            </div>
          </div>
        </div>
      </Show>

      {/* Sequencing footer: continue/skip a rebase, abort any operation. */}
      <Show when={props.conflictState !== "None"}>
        <div
          style={{
            display: "flex",
            gap: "0.5rem",
            padding: "0.4rem 0.6rem",
            "border-top": "1px solid var(--border)",
            "flex-shrink": 0,
            "align-items": "center",
          }}
        >
          <span style={{ "font-size": "0.78rem", color: "var(--fg-muted)" }}>
            In {props.conflictState.toLowerCase()}
          </span>
          <span style={{ flex: "1" }} />
          <Show when={props.conflictState === "Rebase"}>
            <button style={headerBtn} disabled={busy()} onClick={() => runRebaseStep("rebase_continue")}>
              Continue rebase
            </button>
            <button style={headerBtn} disabled={busy()} onClick={() => runRebaseStep("rebase_skip")}>
              Skip commit
            </button>
          </Show>
          <button
            style={{ ...headerBtn, color: "var(--danger)", "border-color": "var(--danger)" }}
            disabled={busy()}
            title={`Abort the ${opLabel()} and discard all conflict resolution`}
            onClick={abandonOp}
          >
            Abandon {opLabel()}
          </button>
        </div>
      </Show>
    </div>
  );
};

export default ConflictResolver;
