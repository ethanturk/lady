import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import hljs from "highlight.js";
import "highlight.js/styles/github.css";
import type { DiffHunk, DiffLine, DiffSpec, FileDiff, RepoId } from "./commands";

type Mode = "unified" | "split";

const ADD_BG = "#e6ffec";
const DEL_BG = "#ffebe9";

/** Map a file extension to a highlight.js language id (best-effort). */
function langFromPath(path: string): string | undefined {
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

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Highlighted HTML for one line of code (escaped fallback when no language). */
function highlight(content: string, lang: string | undefined): string {
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
  left?: DiffLine;
  right?: DiffLine;
}

/**
 * Pair removed/added lines into side-by-side rows. Context flushes any pending
 * removed-line buffer; added lines pair against buffered removed lines first.
 */
function splitRows(lines: DiffLine[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let removed: DiffLine[] = [];
  let addIdx = 0;
  const flush = () => {
    for (const l of removed) rows.push({ left: l });
    removed = [];
  };
  for (const line of lines) {
    if (line.kind === "Deleted") {
      removed.push(line);
    } else if (line.kind === "Added") {
      if (addIdx < removed.length) {
        rows.push({ left: removed[addIdx], right: line });
        addIdx++;
        if (addIdx === removed.length) {
          removed = [];
          addIdx = 0;
        }
      } else {
        rows.push({ right: line });
      }
    } else {
      flush();
      addIdx = 0;
      rows.push({ left: line, right: line });
    }
  }
  // Any leftover removed lines with no matching adds.
  for (let i = addIdx; i < removed.length; i++) rows.push({ left: removed[i] });
  return rows;
}

const monoCell = {
  "font-family": "monospace",
  "font-size": "0.8rem",
  "white-space": "pre" as const,
  padding: "0 0.5rem",
  overflow: "hidden",
};

const HunkSplit: Component<{ hunk: DiffHunk; lang: string | undefined }> = (props) => (
  <For each={splitRows(props.hunk.lines)}>
    {(row) => {
      const lbg = row.left?.kind === "Deleted" ? DEL_BG : "transparent";
      const rbg = row.right?.kind === "Added" ? ADD_BG : "transparent";
      return (
        <div style={{ display: "flex" }}>
          <span
            style={{ ...monoCell, flex: "1", background: lbg, "border-right": "1px solid var(--border)" }}
            innerHTML={row.left ? highlight(row.left.content, props.lang) : "&nbsp;"}
          />
          <span
            style={{ ...monoCell, flex: "1", background: rbg }}
            innerHTML={row.right ? highlight(row.right.content, props.lang) : "&nbsp;"}
          />
        </div>
      );
    }}
  </For>
);

const actionBtn = {
  border: "1px solid var(--border)",
  background: "var(--surface)",
  "border-radius": "3px",
  "font-size": "0.7rem",
  cursor: "pointer",
};

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
}> = (props) => {
  const [sel, setSel] = createSignal<number[]>([]);
  const selectable = () => !!(props.onStageLines || props.onDiscardLines);
  const isSel = (i: number) => sel().includes(i);
  const toggle = (i: number) =>
    setSel((prev) => (prev.includes(i) ? prev.filter((x) => x !== i) : [...prev, i]));
  const clear = () => setSel([]);

  return (
    <div style={{ "border-top": "1px solid var(--border)" }}>
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: "0.4rem",
          background: "var(--surface-2)",
          color: "var(--fg-muted)",
          "font-family": "monospace",
          "font-size": "0.75rem",
          padding: "0.15rem 0.5rem",
        }}
      >
        <span style={{ flex: "1" }}>
          @@ -{props.hunk.old_start},{props.hunk.old_lines} +{props.hunk.new_start},
          {props.hunk.new_lines} @@
        </span>
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
      </div>
      <Show
        when={props.mode === "unified"}
        fallback={<HunkSplit hunk={props.hunk} lang={props.lang} />}
      >
        <For each={props.hunk.lines}>
          {(line, i) => {
            const canSelect = () =>
              selectable() && (line.kind === "Added" || line.kind === "Deleted");
            const bg = () =>
              isSel(i())
                ? "#fff3bf"
                : line.kind === "Added"
                  ? ADD_BG
                  : line.kind === "Deleted"
                    ? DEL_BG
                    : "transparent";
            const sign = line.kind === "Added" ? "+" : line.kind === "Deleted" ? "-" : " ";
            return (
              <div style={{ display: "flex", background: bg() }}>
                <Show
                  when={canSelect()}
                  fallback={
                    <span style={{ ...monoCell, color: "var(--fg-muted)", "min-width": "1.8ch", padding: "0 0.25rem" }}>
                      {sign}
                    </span>
                  }
                >
                  <input
                    type="checkbox"
                    checked={isSel(i())}
                    onChange={() => toggle(i())}
                    style={{ margin: "0 0.25rem", cursor: "pointer" }}
                    title={`${sign} select line`}
                  />
                </Show>
                <span style={{ ...monoCell, flex: "1" }} innerHTML={highlight(line.content, props.lang)} />
              </div>
            );
          }}
        </For>
      </Show>
    </div>
  );
};

const FileBlock: Component<{
  file: FileDiff;
  mode: Mode;
  hunkActionLabel?: string;
  onHunkAction?: (path: string, hunkIndex: number) => void;
  onStageLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardHunk?: (path: string, hunkIndex: number) => void;
  onExternalDiff?: (path: string) => void;
}> = (props) => {
  const lang = () => langFromPath(props.file.path);
  return (
    <div
      style={{
        "margin-bottom": "1rem",
        border: "1px solid var(--border)",
        "border-radius": "4px",
        overflow: "hidden",
        background: "var(--code-bg)",
        color: "var(--code-fg)",
      }}
    >
      <div
        style={{
          padding: "0.4rem 0.6rem",
          background: "var(--surface-2)",
          // Header is chrome, not code — themed text (the block forces dark
          // code-fg for the light code area below).
          color: "var(--fg)",
          "border-bottom": "1px solid var(--border)",
          "font-family": "monospace",
          "font-size": "0.8rem",
          "font-weight": 600,
          display: "flex",
          "align-items": "center",
        }}
      >
        {props.file.path}
        <span style={{ color: "var(--fg-muted)", "font-weight": 400, "margin-left": "0.5rem" }}>
          {props.file.kind}
        </span>
        <span style={{ flex: "1" }} />
        <Show when={props.onExternalDiff}>
          <button style={{ ...actionBtn }} title="Open in external diff tool" onClick={() => props.onExternalDiff!(props.file.path)}>
            Ext diff
          </button>
        </Show>
      </div>
      <Show when={props.file.kind === "Image"}>
        <div style={{ display: "flex", gap: "1rem", padding: "0.6rem", "flex-wrap": "wrap" }}>
          <Show when={props.file.old_image_b64}>
            <figure style={{ margin: 0 }}>
              <figcaption style={{ "font-size": "0.75rem", color: "var(--fg-muted)" }}>old</figcaption>
              <img
                src={`data:${imageMime(props.file.path)};base64,${props.file.old_image_b64}`}
                style={{ "max-width": "300px", "max-height": "300px" }}
              />
            </figure>
          </Show>
          <Show when={props.file.new_image_b64}>
            <figure style={{ margin: 0 }}>
              <figcaption style={{ "font-size": "0.75rem", color: "var(--fg-muted)" }}>new</figcaption>
              <img
                src={`data:${imageMime(props.file.path)};base64,${props.file.new_image_b64}`}
                style={{ "max-width": "300px", "max-height": "300px" }}
              />
            </figure>
          </Show>
        </div>
      </Show>
      <Show when={props.file.kind === "Binary"}>
        <div style={{ padding: "0.6rem", color: "var(--fg-muted)", "font-size": "0.8rem" }}>
          Binary file — no text diff.
        </div>
      </Show>
      <Show when={props.file.hunks.length > 0}>
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
            />
          )}
        </For>
      </Show>
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
  filterPath?: string;
  /** When set, each hunk shows this button calling onHunkAction(path, idx). */
  hunkActionLabel?: string;
  onHunkAction?: (path: string, hunkIndex: number) => void;
  /** When set, changed lines get checkboxes for line-level stage/discard. */
  onStageLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardLines?: (path: string, hunkIndex: number, lines: number[]) => void;
  onDiscardHunk?: (path: string, hunkIndex: number) => void;
}> = (props) => {
  const [files, setFiles] = createSignal<FileDiff[]>([]);
  const [mode, setMode] = createSignal<Mode>("unified");
  const [loading, setLoading] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);

  // Open a file in the user's configured external diff tool (PH3-010). Uses the
  // commit when this is a commit diff; otherwise the working tree.
  const externalDiff = (path: string) => {
    setErr(null);
    invoke("launch_difftool", { repo: props.repoId, path, commit: props.commit ?? null }).catch((e) =>
      setErr(String(e)),
    );
  };

  // A label for the diff header: spec target, else short commit hash.
  const title = () => {
    if (props.spec) return props.spec.value;
    return props.commit ? props.commit.slice(0, 8) : "";
  };

  createEffect(() => {
    const spec = props.spec;
    const commit = props.commit;
    const repo = props.repoId;
    setLoading(true);
    setErr(null);
    const filter = props.filterPath;
    const req = spec
      ? invoke<FileDiff[]>("diff_spec", { repo, spec })
      : invoke<FileDiff[]>("diff", { repo, commit });
    req
      .then((d) => setFiles(filter ? d.filter((f) => f.path === filter) : d))
      .catch((e) => setErr(String(e)))
      .finally(() => setLoading(false));
  });

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column" }}>
      <div
        style={{
          padding: "0.4rem 0.6rem",
          "flex-shrink": 0,
          "border-bottom": "1px solid var(--border)",
          display: "flex",
          "align-items": "center",
          gap: "0.5rem",
        }}
      >
        <span style={{ "font-family": "monospace", "font-size": "0.8rem" }}>
          {title()}
        </span>
        <span style={{ flex: "1" }} />
        <button
          onClick={() => setMode("unified")}
          style={{ "font-weight": mode() === "unified" ? 700 : 400 }}
        >
          Unified
        </button>
        <button
          onClick={() => setMode("split")}
          style={{ "font-weight": mode() === "split" ? 700 : 400 }}
        >
          Split
        </button>
      </div>
      <div style={{ flex: "1", "overflow-y": "auto", padding: "0.6rem" }}>
        <Show when={err()}>
          <p style={{ color: "var(--error)", "font-size": "0.85rem" }}>{err()}</p>
        </Show>
        <Show when={loading()}>
          <p style={{ color: "var(--fg-muted)", "font-size": "0.85rem" }}>Loading diff…</p>
        </Show>
        <Show when={!loading() && files().length === 0 && !err()}>
          <p style={{ color: "var(--fg-muted)", "font-size": "0.85rem" }}>No changes.</p>
        </Show>
        <For each={files()}>
          {(file) => (
            <FileBlock
              file={file}
              mode={mode()}
              hunkActionLabel={props.hunkActionLabel}
              onHunkAction={props.onHunkAction}
              onStageLines={props.onStageLines}
              onDiscardLines={props.onDiscardLines}
              onDiscardHunk={props.onDiscardHunk}
              onExternalDiff={externalDiff}
            />
          )}
        </For>
      </div>
    </div>
  );
};

export default DiffView;
