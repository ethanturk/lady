import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import css from "highlight.js/lib/languages/css";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import markdown from "highlight.js/lib/languages/markdown";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
// Syntax token colors are themed via CSS variables in styles.css (no static
// highlight.js stylesheet, so the diff recolors with the app theme).
import type { DiffHunk, DiffLine, DiffSpec, FileDiff, RepoId } from "./commands";
import { wrapDiff } from "./prefs";

hljs.registerLanguage("bash", bash);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("css", css);
hljs.registerLanguage("go", go);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("python", python);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("yaml", yaml);

type Mode = "unified" | "split";
type DisplayFileDiff = FileDiff & { sourceLabel?: string };

const fileKey = (file: DisplayFileDiff) => `${file.sourceLabel ?? ""}\0${file.path}`;

/** A diff line decorated with its computed old/new line numbers. */
interface NumberedLine {
  line: DiffLine;
  /** Index into hunk.lines (for line-level staging selection). */
  index: number;
  oldNo: number | null;
  newNo: number | null;
}

/** Compute old/new line numbers for a hunk's lines (git supplies only starts). */
function numberLines(hunk: DiffHunk): NumberedLine[] {
  let oldNo = hunk.old_start;
  let newNo = hunk.new_start;
  return hunk.lines.map((line, index) => {
    if (line.kind === "Deleted") return { line, index, oldNo: oldNo++, newNo: null };
    if (line.kind === "Added") return { line, index, oldNo: null, newNo: newNo++ };
    return { line, index, oldNo: oldNo++, newNo: newNo++ };
  });
}

/** Render a single hunk back to a minimal unified-diff patch (for AI explain). */
function hunkToPatch(path: string, h: DiffHunk): string {
  const header = `@@ -${h.old_start},${h.old_lines} +${h.new_start},${h.new_lines} @@`;
  const body = h.lines
    .map((l) => (l.kind === "Added" ? "+" : l.kind === "Deleted" ? "-" : " ") + l.content)
    .join("\n");
  return `--- a/${path}\n+++ b/${path}\n${header}\n${body}`;
}

/** Map a file extension to a highlight.js language id (best-effort). */
export function langFromPath(path: string): string | undefined {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const map: Record<string, string> = {
    rs: "rust",
    ts: "typescript",
    tsx: "typescript",
    js: "javascript",
    jsx: "javascript",
    json: "json",
    toml: "ini",
    md: "markdown",
    py: "python",
    go: "go",
    c: "c",
    h: "c",
    cpp: "cpp",
    hpp: "cpp",
    css: "css",
    html: "xml",
    yaml: "yaml",
    yml: "yaml",
    sh: "bash",
    sql: "sql",
  };
  const lang = map[ext];
  return lang && hljs.getLanguage(lang) ? lang : undefined;
}

export const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Highlighted HTML for one line of code (escaped fallback when no language). */
export function highlight(content: string, lang: string | undefined): string {
  if (!content) return "&nbsp;";
  if (!lang) return escapeHtml(content);
  try {
    return hljs.highlight(content, { language: lang, ignoreIllegals: true }).value;
  } catch {
    return escapeHtml(content);
  }
}

/** Guess an image MIME type from the file extension for data-URL rendering. */
function imageMime(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "jpg" || ext === "jpeg") return "image/jpeg";
  if (ext === "svg") return "image/svg+xml";
  if (ext === "ico") return "image/x-icon";
  return `image/${ext || "png"}`;
}

interface SplitRow {
  left?: NumberedLine;
  right?: NumberedLine;
}

/**
 * Pair removed/added lines into side-by-side rows. Context flushes any pending
 * removed-line buffer; added lines pair against buffered removed lines first.
 */
function splitRows(lines: NumberedLine[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let removed: NumberedLine[] = [];
  let addIdx = 0;
  const flush = () => {
    for (const l of removed) rows.push({ left: l });
    removed = [];
  };
  for (const nl of lines) {
    if (nl.line.kind === "Deleted") {
      removed.push(nl);
    } else if (nl.line.kind === "Added") {
      if (addIdx < removed.length) {
        rows.push({ left: removed[addIdx], right: nl });
        addIdx++;
        if (addIdx === removed.length) {
          removed = [];
          addIdx = 0;
        }
      } else {
        rows.push({ right: nl });
      }
    } else {
      flush();
      addIdx = 0;
      rows.push({ left: nl, right: nl });
    }
  }
  for (let i = addIdx; i < removed.length; i++) rows.push({ left: removed[i] });
  return rows;
}

// ── Row styling (design diff tokens) ───────────────────────────────────────────
const ADD_BG = "var(--diff-add-bg)";
const DEL_BG = "var(--diff-del-bg)";
const ADD_GUT = "rgba(63, 185, 80, 0.22)";
const DEL_GUT = "rgba(229, 83, 75, 0.2)";

// Code cell. When wrapping is off the cell keeps its natural (un-shrunk) width
// so the longest line drives a horizontal scroll; when on it flexes and wraps.
const codeStyle = (wrap: boolean): JSX.CSSProperties => ({
  "font-family": "ui-monospace, 'SF Mono', 'JetBrains Mono', monospace",
  "font-size": "12.5px",
  "white-space": wrap ? "pre-wrap" : "pre",
  "overflow-wrap": wrap ? "anywhere" : "normal",
  padding: "0 8px",
  ...(wrap ? { flex: "1", "min-width": "0" } : { "flex-shrink": "0" }),
});

// Horizontal-scroll scaffold shared by unified rows and each split half. The
// outer scroller clips/scrolls; the inner sizing column grows to the widest
// line (max-content) but never narrower than the viewport so row backgrounds
// span full width. When wrapping is on it collapses to normal flow.
const scrollerStyle = (wrap: boolean): JSX.CSSProperties =>
  wrap ? { "overflow-x": "visible" } : { "overflow-x": "auto" };
const sizerStyle = (wrap: boolean): JSX.CSSProperties =>
  wrap ? { width: "auto", "min-width": "100%" } : { width: "max-content", "min-width": "100%" };

const rowBgFor = (kind: DiffLine["kind"]) =>
  kind === "Added" ? ADD_BG : kind === "Deleted" ? DEL_BG : "transparent";
const codeColorFor = (kind: DiffLine["kind"]) =>
  kind === "Added" ? "var(--diff-add-tx)" : kind === "Deleted" ? "var(--diff-del-tx)" : "var(--difftx)";
const gutBgFor = (kind: DiffLine["kind"]) =>
  kind === "Added" ? ADD_GUT : kind === "Deleted" ? DEL_GUT : "var(--diffgut)";

/** A right-aligned line-number gutter cell. `left` pins it during horizontal
 * scroll so line numbers stay visible when lines run off-screen. */
const Gutter: Component<{ n: number | null; bg: string; left?: string }> = (props) => (
  <span
    style={{
      width: "46px",
      "flex-shrink": "0",
      "text-align": "right",
      "padding-right": "9px",
      color: "var(--lineno)",
      background: props.bg,
      "font-family": "ui-monospace, monospace",
      "font-size": "12px",
      "user-select": "none",
      "line-height": "21px",
      position: "sticky",
      left: props.left ?? "0",
      "z-index": "1",
    }}
  >
    {props.n ?? ""}
  </span>
);

const actionBtn = {
  border: "1px solid var(--bd)",
  background: "var(--btn)",
  color: "var(--tx)",
  "border-radius": "6px",
  "font-size": "11px",
  padding: "2px 8px",
  cursor: "pointer",
};

const HunkHeader: Component<{ hunk: DiffHunk; children?: JSX.Element }> = (props) => (
  <div
    style={{
      display: "flex",
      "align-items": "center",
      gap: "0.4rem",
      background: "var(--hunk-bg)",
      color: "var(--hunk-tx)",
      "font-family": "ui-monospace, monospace",
      "font-size": "12px",
      padding: "2px 10px",
    }}
  >
    <span style={{ flex: "1" }}>
      @@ -{props.hunk.old_start},{props.hunk.old_lines} +{props.hunk.new_start},{props.hunk.new_lines} @@
    </span>
    {props.children}
  </div>
);

const HunkSplit: Component<{ rows: SplitRow[]; lang: string | undefined }> = (props) => (
  <For each={props.rows}>
    {(row) => {
      const half = (nl: NumberedLine | undefined, side: "old" | "new") => {
        const kind = nl?.line.kind;
        const sign = kind === "Added" ? "+" : kind === "Deleted" ? "-" : kind ? " " : "";
        const bg = nl ? rowBgFor(kind!) : "var(--diffgut)";
        return (
          <div style={{ ...scrollerStyle(wrapDiff()), flex: "1", "min-width": "0", background: bg }}>
            <div style={{ ...sizerStyle(wrapDiff()), display: "flex", background: bg }}>
              <Gutter n={nl ? (side === "old" ? nl.oldNo : nl.newNo) : null} bg={nl ? gutBgFor(kind!) : "var(--diffgut)"} />
              <span style={{ width: "18px", "flex-shrink": "0", "text-align": "center", color: codeColorFor(kind ?? "Context"), "font-family": "ui-monospace, monospace", "font-size": "12px", "line-height": "21px", position: "sticky", left: "46px", "z-index": "1", background: bg }}>{sign}</span>
              <span style={{ ...codeStyle(wrapDiff()), color: codeColorFor(kind ?? "Context") }} innerHTML={nl ? highlight(nl.line.content, props.lang) : "&nbsp;"} />
            </div>
          </div>
        );
      };
      return (
        <div style={{ display: "flex", "min-height": "21px" }}>
          <div style={{ display: "flex", flex: "1", "min-width": "0", "border-right": "1px solid var(--bd)" }}>{half(row.left, "old")}</div>
          <div style={{ display: "flex", flex: "1", "min-width": "0" }}>{half(row.right, "new")}</div>
        </div>
      );
    }}
  </For>
);

/**
 * One hunk: header (with hunk- and line-level actions) + body. When line
 * actions are supplied (the unstaged Changes view), each changed line gets a
 * checkbox so a subset can be staged or discarded.
 */
const HunkBlock: Component<{
  path: string;
  hunk: DiffHunk;
  hunkIndex: number;
  mode: Mode;
  lang: string | undefined;
  hunkActionLabel?: string;
  onHunkAction?: (path: string, hunkIndex: number) => void;
  onStageLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardHunk?: (path: string, hunkIndex: number) => void;
  onExplainHunk?: (path: string, patch: string) => void;
}> = (props) => {
  const [sel, setSel] = createSignal<number[]>([]);
  const selectable = () => !!(props.onStageLines || props.onDiscardLines);
  const isSel = (i: number) => sel().includes(i);
  const toggle = (i: number) =>
    setSel((prev) => (prev.includes(i) ? prev.filter((x) => x !== i) : [...prev, i]));
  const clear = () => setSel([]);
  const numbered = createMemo(() => numberLines(props.hunk));

  return (
    <div style={{ "border-top": "1px solid var(--bd)" }}>
      <HunkHeader hunk={props.hunk}>
        <Show when={sel().length > 0 && props.onStageLines}>
          <button style={actionBtn} onClick={() => { props.onStageLines!(props.path, props.hunkIndex, sel()); clear(); }}>
            Stage {sel().length} line{sel().length > 1 ? "s" : ""}
          </button>
        </Show>
        <Show when={sel().length > 0 && props.onDiscardLines}>
          <button
            style={actionBtn}
            onClick={() => {
              if (!confirm(`Discard ${sel().length} line(s)? This cannot be undone.`)) return;
              props.onDiscardLines!(props.path, props.hunkIndex, sel());
              clear();
            }}
          >
            Discard {sel().length} line{sel().length > 1 ? "s" : ""}
          </button>
        </Show>
        <Show when={props.onHunkAction}>
          <button style={actionBtn} onClick={() => props.onHunkAction!(props.path, props.hunkIndex)}>
            {props.hunkActionLabel ?? "Stage hunk"}
          </button>
        </Show>
        <Show when={props.onDiscardHunk}>
          <button
            style={actionBtn}
            onClick={() => {
              if (!confirm("Discard this hunk? This cannot be undone.")) return;
              props.onDiscardHunk!(props.path, props.hunkIndex);
            }}
          >
            Discard hunk
          </button>
        </Show>
        <Show when={props.onExplainHunk}>
          <button style={actionBtn} title="Explain this hunk with AI" onClick={() => props.onExplainHunk!(props.path, hunkToPatch(props.path, props.hunk))}>
            Explain changes
          </button>
        </Show>
      </HunkHeader>
      <Show
        when={props.mode === "unified"}
        fallback={<HunkSplit rows={splitRows(numbered())} lang={props.lang} />}
      >
        <div style={scrollerStyle(wrapDiff())}>
          <div style={sizerStyle(wrapDiff())}>
            <For each={numbered()}>
              {(nl) => {
                const kind = nl.line.kind;
                const canSelect = () => selectable() && (kind === "Added" || kind === "Deleted");
                const sign = kind === "Added" ? "+" : kind === "Deleted" ? "-" : " ";
                const bg = () => (isSel(nl.index) ? "var(--diff-sel-bg)" : rowBgFor(kind));
                return (
                  <div style={{ display: "flex", "min-height": "21px", background: bg() }}>
                    <Gutter n={nl.oldNo} bg={gutBgFor(kind)} left="0" />
                    <Gutter n={nl.newNo} bg={gutBgFor(kind)} left="46px" />
                    <Show
                      when={canSelect()}
                      fallback={
                        <span style={{ width: "20px", "flex-shrink": "0", "text-align": "center", color: codeColorFor(kind), "font-family": "ui-monospace, monospace", "font-size": "12px", "line-height": "21px", position: "sticky", left: "92px", "z-index": "1", background: bg() }}>
                          {sign}
                        </span>
                      }
                    >
                      <input
                        type="checkbox"
                        checked={isSel(nl.index)}
                        onChange={() => toggle(nl.index)}
                        style={{ width: "20px", "flex-shrink": "0", margin: "0", cursor: "pointer", position: "sticky", left: "92px", "z-index": "1", background: bg() }}
                        title={`${sign} select line`}
                      />
                    </Show>
                    <span style={{ ...codeStyle(wrapDiff()), color: codeColorFor(kind) }} innerHTML={highlight(nl.line.content, props.lang)} />
                  </div>
                );
              }}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
};

/** One side's cell for a single split row (no per-cell scroller — the whole
 * column scrolls as one). `nl` is undefined for the blank side of an add/del. */
const SplitHalfCell: Component<{ nl: NumberedLine | undefined; side: "old" | "new"; lang: string | undefined }> = (props) => {
  const kind = () => props.nl?.line.kind;
  const sign = () => (kind() === "Added" ? "+" : kind() === "Deleted" ? "-" : kind() ? " " : "");
  const bg = () => (props.nl ? rowBgFor(kind()!) : "var(--diffgut)");
  return (
    <div style={{ display: "flex", "min-height": "21px", background: bg() }}>
      <Gutter n={props.nl ? (props.side === "old" ? props.nl.oldNo : props.nl.newNo) : null} bg={props.nl ? gutBgFor(kind()!) : "var(--diffgut)"} />
      <span style={{ width: "18px", "flex-shrink": "0", "text-align": "center", color: codeColorFor(kind() ?? "Context"), "font-family": "ui-monospace, monospace", "font-size": "12px", "line-height": "21px", position: "sticky", left: "46px", "z-index": "1", background: bg() }}>{sign()}</span>
      <span style={{ ...codeStyle(false), color: codeColorFor(kind() ?? "Context") }} innerHTML={props.nl ? highlight(props.nl.line.content, props.lang) : "&nbsp;"} />
    </div>
  );
};

/** A split hunk-header bar for one column. The colored bar spans the full
 * (scrolled) width; its content is pinned to the left so the @@ range / action
 * buttons stay visible while the column scrolls horizontally. */
const SplitHeaderBar: Component<{ children?: JSX.Element }> = (props) => (
  <div style={{ "min-width": "100%", height: "25px", display: "flex", "align-items": "center", background: "var(--hunk-bg)", color: "var(--hunk-tx)", "border-top": "1px solid var(--bd)" }}>
    <div style={{ position: "sticky", left: "0", display: "flex", "align-items": "center", gap: "0.4rem", padding: "0 10px", "font-family": "ui-monospace, monospace", "font-size": "12px", "white-space": "nowrap" }}>
      {props.children}
    </div>
  </div>
);

/**
 * Split (side-by-side) diff for a whole file: two independently-scrolling
 * columns (old | new), each a single horizontal scroller spanning every hunk —
 * so there is exactly one scrollbar per side, not one per hunk/row. Hunk headers
 * are split across the columns (range on the left, action buttons on the right)
 * at a fixed height so the two columns stay vertically aligned. Used only when
 * line-wrapping is off; with wrap on, the per-hunk paired {@link HunkSplit} keeps
 * wrapped rows aligned and needs no horizontal scroll.
 */
const FileSplit: Component<{
  file: DisplayFileDiff;
  lang: string | undefined;
  hunkActionLabel?: string;
  onHunkAction?: (path: string, hunkIndex: number) => void;
  onDiscardHunk?: (path: string, hunkIndex: number) => void;
  onExplainHunk?: (path: string, patch: string) => void;
}> = (props) => {
  const hunkRows = createMemo(() =>
    props.file.hunks.map((hunk) => ({ hunk, rows: splitRows(numberLines(hunk)) })),
  );
  const colStyle: JSX.CSSProperties = { "overflow-x": "auto", flex: "1", "min-width": "0" };
  const sizerStyle: JSX.CSSProperties = { width: "max-content", "min-width": "100%" };
  return (
    <div style={{ display: "flex" }}>
      {/* OLD side */}
      <div style={{ ...colStyle, "border-right": "1px solid var(--bd)" }}>
        <div style={sizerStyle}>
          <For each={hunkRows()}>
            {(hr) => (
              <>
                <SplitHeaderBar>
                  @@ -{hr.hunk.old_start},{hr.hunk.old_lines} +{hr.hunk.new_start},{hr.hunk.new_lines} @@
                </SplitHeaderBar>
                <For each={hr.rows}>{(row) => <SplitHalfCell nl={row.left} side="old" lang={props.lang} />}</For>
              </>
            )}
          </For>
        </div>
      </div>
      {/* NEW side */}
      <div style={colStyle}>
        <div style={sizerStyle}>
          <For each={hunkRows()}>
            {(hr, hi) => (
              <>
                <SplitHeaderBar>
                  <Show when={props.onHunkAction}>
                    <button style={actionBtn} onClick={() => props.onHunkAction!(props.file.path, hi())}>
                      {props.hunkActionLabel ?? "Stage hunk"}
                    </button>
                  </Show>
                  <Show when={props.onDiscardHunk}>
                    <button
                      style={actionBtn}
                      onClick={() => {
                        if (!confirm("Discard this hunk? This cannot be undone.")) return;
                        props.onDiscardHunk!(props.file.path, hi());
                      }}
                    >
                      Discard hunk
                    </button>
                  </Show>
                  <Show when={props.onExplainHunk}>
                    <button style={actionBtn} title="Explain this hunk with AI" onClick={() => props.onExplainHunk!(props.file.path, hunkToPatch(props.file.path, hr.hunk))}>
                      Explain changes
                    </button>
                  </Show>
                </SplitHeaderBar>
                <For each={hr.rows}>{(row) => <SplitHalfCell nl={row.right} side="new" lang={props.lang} />}</For>
              </>
            )}
          </For>
        </div>
      </div>
    </div>
  );
};

const FileBlock: Component<{
  file: DisplayFileDiff;
  mode: Mode;
  collapsible?: boolean;
  collapsed: boolean;
  onToggle: () => void;
  hunkActionLabel?: string;
  onHunkAction?: (path: string, hunkIndex: number) => void;
  onStageLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardHunk?: (path: string, hunkIndex: number) => void;
  onExternalDiff?: (path: string) => void;
  onExplainHunk?: (path: string, patch: string) => void;
}> = (props) => {
  const lang = () => langFromPath(props.file.path);
  return (
    <div style={{ "margin-bottom": "16px", border: "1px solid var(--bd)", "border-radius": "7px", overflow: "hidden", background: "var(--code-bg)" }}>
      <div
        style={{
          display: "flex",
          "align-items": "center",
          padding: "0 12px",
          height: "34px",
          background: "var(--sub)",
          color: "var(--tx)",
          "border-bottom": "1px solid var(--bd)",
          "font-family": "ui-monospace, monospace",
          "font-size": "12.5px",
          "font-weight": 600,
        }}
      >
        <Show when={props.collapsible}>
          <button
            onClick={props.onToggle}
            title={props.collapsed ? "Open file diff" : "Close file diff"}
            aria-label={props.collapsed ? "Open file diff" : "Close file diff"}
            aria-expanded={!props.collapsed}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--tx3)",
              "font-family": "ui-monospace, monospace",
              "font-size": "13px",
              padding: "0 7px 0 0",
              cursor: "pointer",
            }}
          >
            {props.collapsed ? "▸" : "▾"}
          </button>
        </Show>
        {props.file.path}
        <Show when={props.file.sourceLabel}>
          <span style={{ color: "var(--tx4)", "font-weight": 400, "margin-left": "8px" }}>{props.file.sourceLabel}</span>
        </Show>
        <span style={{ color: "var(--tx3)", "font-weight": 400, "margin-left": "8px" }}>{props.file.kind}</span>
        <span style={{ flex: "1" }} />
        <Show when={props.onExternalDiff}>
          <button style={actionBtn} title="Open in external diff tool" onClick={() => props.onExternalDiff!(props.file.path)}>
            Ext diff
          </button>
        </Show>
      </div>
      <Show when={!props.collapsed}>
        <Show when={props.file.kind === "Image"}>
          <div style={{ display: "flex", gap: "1rem", padding: "0.6rem", "flex-wrap": "wrap" }}>
            <Show when={props.file.old_image_b64}>
              <figure style={{ margin: 0 }}>
                <figcaption style={{ "font-size": "0.75rem", color: "var(--tx3)" }}>old</figcaption>
                <img src={`data:${imageMime(props.file.path)};base64,${props.file.old_image_b64}`} style={{ "max-width": "300px", "max-height": "300px" }} />
              </figure>
            </Show>
            <Show when={props.file.new_image_b64}>
              <figure style={{ margin: 0 }}>
                <figcaption style={{ "font-size": "0.75rem", color: "var(--tx3)" }}>new</figcaption>
                <img src={`data:${imageMime(props.file.path)};base64,${props.file.new_image_b64}`} style={{ "max-width": "300px", "max-height": "300px" }} />
              </figure>
            </Show>
          </div>
        </Show>
        <Show when={props.file.kind === "Binary"}>
          <div style={{ padding: "0.6rem", color: "var(--tx3)", "font-size": "0.8rem" }}>Binary file — no text diff.</div>
        </Show>
        <Show when={props.file.has_null_bytes && props.file.kind !== "Binary" && props.file.kind !== "Image"}>
          <div style={{ padding: "0.5rem 0.6rem", color: "var(--warning)", "background-color": "var(--warning-bg)", border: "1px solid var(--warning-border)", "font-size": "0.8rem" }}>
            Contains NUL bytes; showing text diff because file content is valid UTF-8.
          </div>
        </Show>
        <Show when={props.file.hunks.length > 0}>
          {/* Split + no-wrap renders the whole file as two columns with one
              scrollbar per side; every other case keeps the per-hunk blocks. */}
          <Show
            when={props.mode === "split" && !wrapDiff()}
            fallback={
              <For each={props.file.hunks}>
                {(hunk, hunkIndex) => (
                  <HunkBlock
                    path={props.file.path}
                    hunk={hunk}
                    hunkIndex={hunkIndex()}
                    mode={props.mode}
                    lang={lang()}
                    hunkActionLabel={props.hunkActionLabel}
                    onHunkAction={props.onHunkAction}
                    onStageLines={props.onStageLines}
                    onDiscardLines={props.onDiscardLines}
                    onDiscardHunk={props.onDiscardHunk}
                    onExplainHunk={props.onExplainHunk}
                  />
                )}
              </For>
            }
          >
            <FileSplit
              file={props.file}
              lang={lang()}
              hunkActionLabel={props.hunkActionLabel}
              onHunkAction={props.onHunkAction}
              onDiscardHunk={props.onDiscardHunk}
              onExplainHunk={props.onExplainHunk}
            />
          </Show>
        </Show>
      </Show>
    </div>
  );
};

// Height (px) a FileBlock occupies below its box (its margin-bottom). Placeholder
// spacers add it so the scroll height matches a mounted file exactly.
const FILE_GAP = 16;
const LINE_H = 21;

/**
 * Cheap height estimate for an unmounted file, derived from the diff data (no
 * layout). Keeps the scrollbar roughly correct before a file has ever been
 * measured, so placeholders don't collapse the scroll range on a large diff.
 */
function estimateFileHeight(file: DisplayFileDiff): number {
  const header = 34;
  if (file.kind === "Image") return header + 340 + FILE_GAP;
  if (file.kind === "Binary") return header + 40 + FILE_GAP;
  if (file.hunks.length === 0) return header + 24 + FILE_GAP;
  let body = 0;
  for (const h of file.hunks) body += 26 + h.lines.length * LINE_H;
  return header + body + FILE_GAP;
}

/**
 * File-level virtualization. Each file renders only while it (or an 800px margin
 * around it) intersects the diff scroller; off-screen files collapse to a
 * placeholder holding their last-measured (or estimated) height. This keeps the
 * mounted DOM — and the per-line syntax highlighting — proportional to what's on
 * screen instead of the whole diff, which is what made large diffs scroll-drag.
 * The outer wrapper is always mounted, so scroll-to-file offsets stay valid.
 */
const VirtualFile: Component<{
  scrollRoot: () => HTMLElement | undefined;
  estimate: number;
  registerEl: (el: HTMLDivElement) => void;
  children: JSX.Element;
}> = (props) => {
  const [visible, setVisible] = createSignal(false);
  const [measured, setMeasured] = createSignal<number | null>(null);
  let wrap!: HTMLDivElement;

  onMount(() => {
    props.registerEl(wrap);
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) setVisible(e.isIntersecting);
      },
      { root: props.scrollRoot() ?? null, rootMargin: "800px 0px" },
    );
    io.observe(wrap);
    // Track the mounted content's real height so the placeholder holds the exact
    // space when it unmounts (covers collapse, wrap toggle, and image loads).
    const ro = new ResizeObserver(() => {
      if (!visible() || !wrap) return;
      const h = wrap.getBoundingClientRect().height;
      if (h > 0) setMeasured(h);
    });
    ro.observe(wrap);
    onCleanup(() => {
      io.disconnect();
      ro.disconnect();
    });
  });

  const spacerH = () => measured() ?? props.estimate;
  return (
    <div ref={wrap} style={visible() ? undefined : { height: `${spacerH()}px` }}>
      <Show when={visible()}>{props.children}</Show>
    </div>
  );
};

/** Segmented Unified | Split control (design diff header). */
const Segmented: Component<{ mode: Mode; onMode: (m: Mode) => void }> = (props) => {
  const cell = (m: Mode, label: string, first: boolean) => {
    const on = () => props.mode === m;
    return (
      <button
        onClick={() => props.onMode(m)}
        style={{
          padding: "4px 12px",
          border: "none",
          "border-left": first ? "none" : "1px solid var(--bd)",
          background: on() ? "var(--accent)" : "transparent",
          color: on() ? "var(--on-accent-strong)" : "var(--tx3)",
          "font-size": "11px",
          "font-weight": on() ? 600 : 400,
          cursor: "pointer",
        }}
        aria-pressed={on()}
      >
        {label}
      </button>
    );
  };
  return (
    <div style={{ display: "inline-flex", border: "1px solid var(--bd)", "border-radius": "7px", overflow: "hidden" }}>
      {cell("unified", "Unified", true)}
      {cell("split", "Split", false)}
    </div>
  );
};

/**
 * Renders a diff. Either a `commit` (its diff vs first parent, Phase 1) or a
 * `spec` (working-vs-index / index-vs-HEAD for a single file, Phase 2) drives
 * the fetch; `spec` takes precedence when both are given.
 */
const DiffView: Component<{
  repoId: RepoId;
  commit?: string;
  spec?: DiffSpec;
  files?: DisplayFileDiff[];
  title?: string;
  filterPath?: string;
  collapsible?: boolean;
  scrollToPath?: string;
  scrollKey?: unknown;
  /** Bump to force a refetch when the underlying diff changed but the spec did
   * not (e.g. after staging a hunk of the currently-selected file). */
  refreshKey?: unknown;
  /** When set, each hunk shows this button calling onHunkAction(path, idx). */
  hunkActionLabel?: string;
  /** When set, changed lines get checkboxes for line-level stage/discard. */
  onStageLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardHunk?: (path: string, hunkIndex: number) => void;
  onHunkAction?: (path: string, hunkIndex: number) => void;
  /** When set, each hunk shows an "Explain changes" button (AI). */
  onExplainHunk?: (path: string, patch: string) => void;
}> = (props) => {
  const [files, setFiles] = createSignal<DisplayFileDiff[]>([]);
  const [mode, setMode] = createSignal<Mode>("unified");
  const [loading, setLoading] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const [collapsed, setCollapsed] = createSignal<Set<string>>(new Set());

  let scrollEl!: HTMLDivElement;
  const fileEls = new Map<string, HTMLDivElement>();

  const toggleFile = (key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      next.has(key) ? next.delete(key) : next.add(key);
      return next;
    });
  };

  // Open a file in the user's configured external diff tool (PH3-010).
  const externalDiff = (path: string) => {
    setErr(null);
    invoke("launch_difftool", { repo: props.repoId, path, commit: props.commit ?? null }).catch((e) => setErr(String(e)));
  };

  const title = () => {
    if (props.title) return props.title;
    if (props.spec) return props.spec.value;
    return props.commit ? props.commit.slice(0, 8) : "";
  };

  createEffect(() => {
    const suppliedFiles = props.files;
    const spec = props.spec;
    const commit = props.commit;
    const repo = props.repoId;
    void props.refreshKey; // refetch when the diff changed under a stable spec
    setLoading(true);
    setErr(null);
    fileEls.clear();
    const filter = props.filterPath;
    if (suppliedFiles) {
      setFiles(filter ? suppliedFiles.filter((f) => f.path === filter) : suppliedFiles);
      setLoading(false);
      return;
    }
    const req = spec
      ? invoke<FileDiff[]>("diff_spec", { repo, spec })
      : invoke<FileDiff[]>("diff", { repo, commit });
    req
      .then((d) => setFiles(filter ? d.filter((f) => f.path === filter) : d))
      .catch((e) => setErr(String(e)))
      .finally(() => setLoading(false));
  });

  createEffect(() => {
    const target = props.scrollToPath;
    void props.scrollKey;
    if (!target) return;
    setCollapsed((prev) => {
      const next = new Set(prev);
      for (const file of files()) {
        if (file.path === target) next.delete(fileKey(file));
      }
      return next;
    });
    requestAnimationFrame(() => {
      const el = fileEls.get(target);
      if (!el || !scrollEl) return;
      scrollEl.scrollTo({ top: Math.max(0, el.offsetTop - 10), behavior: "smooth" });
    });
  });

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column" }}>
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: "0.5rem",
          height: "34px",
          padding: "0 12px 0 16px",
          "flex-shrink": 0,
          background: "var(--sub)",
          "border-bottom": "1px solid var(--bd)",
        }}
      >
        <span style={{ "font-family": "ui-monospace, monospace", "font-size": "12.5px", color: "var(--tx2)", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
          {title()}
        </span>
        <span style={{ flex: "1" }} />
        <Segmented mode={mode()} onMode={setMode} />
      </div>
      <div ref={scrollEl} class="scroll-thin" style={{ flex: "1", "overflow-y": "auto", padding: "12px", background: "var(--bg)" }}>
        <Show when={err()}>
          <p style={{ color: "var(--error)", "font-size": "0.85rem" }}>{err()}</p>
        </Show>
        <Show when={loading()}>
          <p style={{ color: "var(--tx3)", "font-size": "0.85rem" }}>Loading diff…</p>
        </Show>
        <Show when={!loading() && files().length === 0 && !err()}>
          <p style={{ color: "var(--tx3)", "font-size": "0.85rem" }}>No changes.</p>
        </Show>
        <For each={files()}>
          {(file) => (
            <VirtualFile
              scrollRoot={() => scrollEl}
              estimate={estimateFileHeight(file)}
              registerEl={(el) => { if (!fileEls.has(file.path)) fileEls.set(file.path, el); }}
            >
              <FileBlock
                file={file}
                mode={mode()}
                collapsible={props.collapsible}
                collapsed={props.collapsible ? collapsed().has(fileKey(file)) : false}
                onToggle={() => toggleFile(fileKey(file))}
                hunkActionLabel={props.hunkActionLabel}
                onHunkAction={props.onHunkAction}
                onStageLines={props.onStageLines}
                onDiscardLines={props.onDiscardLines}
                onDiscardHunk={props.onDiscardHunk}
                onExternalDiff={props.files && !props.commit && !props.spec ? undefined : externalDiff}
                onExplainHunk={props.onExplainHunk}
              />
            </VirtualFile>
          )}
        </For>
      </div>
    </div>
  );
};

export default DiffView;
