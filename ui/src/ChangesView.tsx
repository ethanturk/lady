import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ChangeKind, DiffSpec, FileStatus, RepoId, WorkingTree } from "./commands";
import DiffView from "./DiffView";
import ContextMenu from "./ContextMenu";
import type { MenuEntry } from "./ContextMenu";
import { cancelAi, isConsentError, runAiStream } from "./ai";
import { changesColWidth, changesLayout, setChangesColWidth, setStagedHeight, stagedHeight } from "./prefs";
import { effectiveSign, repoSettings } from "./repoSettings";
import type { ActionResult } from "./branchActions";
import { copyPath, discardChanges, ignore, openFile, revealFile, saveAsPatch, stashFiles } from "./fileActions";

/** A 17px colored status badge for a file's change kind (design tokens). */
const KIND_BADGE: Record<ChangeKind, { label: string; bg: string; fg: string }> = {
  Added: { label: "A", bg: "var(--badge-a)", fg: "var(--on-badge)" },
  Modified: { label: "M", bg: "var(--badge-m)", fg: "var(--on-badge)" },
  Deleted: { label: "D", bg: "var(--badge-d)", fg: "var(--on-badge)" },
  Renamed: { label: "R", bg: "var(--badge-r)", fg: "var(--on-badge)" },
  Untracked: { label: "?", bg: "var(--tx4)", fg: "var(--on-badge)" },
  Conflicted: { label: "!", bg: "var(--danger)", fg: "var(--on-accent)" },
};

const Badge: Component<{ kind: ChangeKind }> = (props) => {
  const b = () => KIND_BADGE[props.kind];
  return (
    <span
      style={{
        display: "inline-flex",
        "align-items": "center",
        "justify-content": "center",
        width: "17px",
        height: "17px",
        "flex-shrink": "0",
        "border-radius": "4px",
        "font-family": "ui-monospace, monospace",
        "font-weight": 700,
        "font-size": "10px",
        background: b().bg,
        color: b().fg,
      }}
      title={props.kind}
    >
      {b().label}
    </span>
  );
};

const subBtn = {
  border: "1px solid var(--bd)",
  background: "var(--btn)",
  color: "var(--tx)",
  "border-radius": "6px",
  "font-size": "12px",
  padding: "4px 14px",
  cursor: "pointer",
};

const accentFill = "color-mix(in srgb, var(--accent) 16%, transparent)";

/** Scale a px padding by the global density step (--pad-scale). */
const ps = (px: number) => `calc(${px}px * var(--pad-scale))`;

/** Which file + side the diff pane is showing. */
interface Selection {
  path: string;
  staged: boolean;
}

interface ChangesViewProps {
  repoId: RepoId;
  /** Bump to force a status reload after an external mutation. */
  refreshNonce?: number;
  /** Called after a mutation here so sibling views (refs/graph) can reload. */
  onChanged?: () => void;
  /** Surface a file-action result (toast / error) in the app shell. */
  onResult?: (r: ActionResult | null) => void;
  /** Open the Blame overlay focused on a path (file menu). */
  onOpenBlame?: (path: string) => void;
  /** Open the File-History overlay focused on a path (file menu). */
  onOpenHistory?: (path: string) => void;
  /** Open the AI "Explain changes" overlay for an ai_explain target. */
  onExplain?: (target: Record<string, unknown>, title: string, subtitle?: string) => void;
}

/**
 * The Local Changes view (design): a 308px file-lists column (Unstaged / Staged
 * with bulk actions), a diff viewer that fills the rest, and a commit composer
 * pinned to the bottom (subject + description + amend + Commit(N)). All staging,
 * stash, and AI handlers are preserved from the previous tabbed layout.
 */
const ChangesView: Component<ChangesViewProps> = (props) => {
  const [wt, setWt] = createSignal<WorkingTree | null>(null);
  const [err, setErr] = createSignal<string | null>(null);
  const [selected, setSelected] = createSignal<Selection | null>(null);
  const [subject, setSubject] = createSignal("");
  const [body, setBody] = createSignal("");
  const [amend, setAmend] = createSignal(false);
  const [sign, setSign] = createSignal(false);
  // Bumped after each staging mutation so the diff pane refetches even when the
  // selected file (and thus its spec) is unchanged.
  const [diffNonce, setDiffNonce] = createSignal(0);
  const [recent, setRecent] = createSignal<string[]>([]);
  // AI commit-message generation (PH5-006).
  const [aiBusy, setAiBusy] = createSignal(false);
  const [aiReq, setAiReq] = createSignal<string | null>(null);

  const message = () => {
    const b = body().trim();
    return b ? `${subject().trim()}\n\n${b}` : subject().trim();
  };

  const generateMessage = async () => {
    if (aiBusy()) return;
    setErr(null);
    setAiBusy(true);
    setSubject("");
    setBody("");
    try {
      const full = await runAiStream(
        "ai_commit_message",
        { repo: props.repoId },
        (acc) => {
          // Stream the first line into the subject, the rest into the body.
          const nl = acc.indexOf("\n");
          if (nl === -1) setSubject(acc);
          else {
            setSubject(acc.slice(0, nl));
            setBody(acc.slice(nl + 1).replace(/^\n/, ""));
          }
        },
        (id) => setAiReq(id),
      );
      const nl = full.indexOf("\n");
      setSubject(nl === -1 ? full : full.slice(0, nl));
      setBody(nl === -1 ? "" : full.slice(nl + 1).replace(/^\n/, ""));
    } catch (e) {
      const msg = String(e);
      setErr(isConsentError(msg) ? "AI consent required — enable the provider and grant consent in Settings." : msg);
    } finally {
      setAiBusy(false);
      setAiReq(null);
    }
  };

  const cancelGenerate = () => {
    const id = aiReq();
    if (id) cancelAi(id).catch(() => {});
  };

  const reload = () => {
    setErr(null);
    invoke<WorkingTree>("status", { repo: props.repoId }).then(setWt).catch((e) => setErr(String(e)));
    invoke<string[]>("recent_messages", { repo: props.repoId, limit: 10 }).then(setRecent).catch(() => setRecent([]));
  };

  const selectedSpec = (): DiffSpec | null => {
    const s = selected();
    if (!s) return null;
    return { kind: s.staged ? "IndexVsHead" : "WorkingVsIndex", value: s.path };
  };
  const isSelected = (path: string, staged: boolean) =>
    selected()?.path === path && selected()?.staged === staged;

  createEffect(() => {
    void props.repoId;
    void props.refreshNonce;
    reload();
  });

  // Seed the commit-signing checkbox from the effective default (per-repo
  // override ?? global default ?? off) whenever the active repo changes. The
  // user can still toggle it per commit.
  createEffect(() => {
    const repo = props.repoId;
    repoSettings(repo)
      .then((r) => setSign(effectiveSign(r)))
      .catch(() => {});
  });

  const afterMutation = () => {
    reload();
    // The selected file's spec is unchanged, so bump a nonce to force the diff
    // pane to refetch (a staged/discarded hunk then disappears from it).
    setDiffNonce((n) => n + 1);
    props.onChanged?.();
  };

  // When the currently-viewed file is staged/unstaged, advance the diff pane to
  // the next file in the same list (else the previous, else clear) so the user
  // keeps moving without re-clicking.
  const advanceSelection = (path: string, fromStaged: boolean) => {
    const sel = selected();
    if (!sel || sel.path !== path || sel.staged !== fromStaged) return;
    const list = fromStaged ? (wt()?.staged ?? []) : unstagedAll();
    const idx = list.findIndex((f) => f.path === path);
    const nextFile = list[idx + 1] ?? list[idx - 1] ?? null;
    setSelected(nextFile ? { path: nextFile.path, staged: fromStaged } : null);
  };

  // Stage/unstage a single file, advancing the selection to the next one.
  const stageOne = (path: string, fromStaged: boolean) => {
    advanceSelection(path, fromStaged);
    fromStaged ? unstage([path]) : stage([path]);
  };

  const stage = (paths: string[]) => {
    if (paths.length === 0) return;
    invoke("stage_paths", { repo: props.repoId, paths }).then(afterMutation).catch((e) => setErr(String(e)));
  };
  const unstage = (paths: string[]) => {
    if (paths.length === 0) return;
    invoke("unstage_paths", { repo: props.repoId, paths }).then(afterMutation).catch((e) => setErr(String(e)));
  };
  const stageHunk = (path: string, hunk: number) => {
    invoke("stage_hunks", { repo: props.repoId, path, hunks: [hunk] }).then(afterMutation).catch((e) => setErr(String(e)));
  };
  const unstageHunk = (path: string, hunk: number) => {
    invoke("unstage_hunks", { repo: props.repoId, path, hunks: [hunk] }).then(afterMutation).catch((e) => setErr(String(e)));
  };
  const stageLines = (path: string, hunk: number, lines: number[]) => {
    invoke("stage_lines", { repo: props.repoId, path, hunk, lines }).then(afterMutation).catch((e) => setErr(String(e)));
  };
  const discardLines = (path: string, hunk: number, lines: number[]) => {
    invoke("discard_lines", { repo: props.repoId, path, hunk, lines }).then(afterMutation).catch((e) => setErr(String(e)));
  };
  const discardHunk = (path: string, hunk: number) => {
    invoke("discard_hunks", { repo: props.repoId, path, hunks: [hunk] }).then(afterMutation).catch((e) => setErr(String(e)));
  };

  // ── File / folder context menu ────────────────────────────────────────────
  const [menu, setMenu] = createSignal<{ x: number; y: number; items: MenuEntry[] } | null>(null);

  // Run a file action wrapper, surface its result, then reload local status.
  const runAction = async (p: Promise<ActionResult | null>) => {
    const r = await p;
    props.onResult?.(r);
    if (r?.ok) afterMutation();
  };
  const externalDiff = (path: string) => {
    invoke("launch_difftool", { repo: props.repoId, path, commit: null }).catch((e) => setErr(String(e)));
  };

  // Every path under a folder `prefix` in the given bucket (for folder menus).
  const subtreePaths = (files: FileStatus[], prefix: string): string[] =>
    files.filter((f) => f.path.startsWith(`${prefix}/`)).map((f) => f.path);

  // Menu for a single file row. `staged` controls the Stage/Unstage verb.
  const fileMenuItems = (f: FileStatus, staged: boolean): MenuEntry[] => {
    const path = f.path;
    const all = staged ? allStagedPaths() : allUnstagedPaths();
    return [
      { label: "Open", run: () => runAction(openFile(props.repoId, path)) },
      { label: "Show in Finder", run: () => runAction(revealFile(props.repoId, path)) },
      { label: "External Diff", shortcut: "⌘D", run: () => externalDiff(path) },
      { label: "Blame / Timeline…", run: () => props.onOpenBlame?.(path) },
      { label: "History…", run: () => props.onOpenHistory?.(path) },
      { label: "Explain changes", run: () => props.onExplain?.({ kind: "path", path }, `Explain ${path}`) },
      "divider",
      staged
        ? { label: "Unstage", shortcut: "⌘S", run: async () => stageOne(path, true) }
        : { label: "Stage", shortcut: "⌘S", run: async () => stageOne(path, false) },
      { label: "Discard Changes…", shortcut: "⇧⌘D", disabled: f.kind === "Untracked", run: () => runAction(discardChanges(props.repoId, [path])) },
      { label: "Stage All", shortcut: "⌥⌘S", run: async () => stage(all) },
      "divider",
      { label: "Ignore", run: () => runAction(ignore(props.repoId, [path])) },
      { label: "Stash 1 File…", run: () => runAction(stashFiles(props.repoId, [path])) },
      { label: "Save as Patch…", run: () => runAction(saveAsPatch(props.repoId, [path])) },
      { label: "Copy Path", shortcut: "⌘C", run: () => runAction(copyPath(path)) },
    ];
  };

  // Menu for a folder row — acts on the whole subtree; per-file actions (External
  // Diff, Blame) are disabled because they need a single file.
  const folderMenuItems = (prefix: string, staged: boolean): MenuEntry[] => {
    const bucket = staged ? (wt()?.staged ?? []) : unstagedAll();
    const paths = subtreePaths(bucket, prefix);
    return [
      { label: "Open", run: () => runAction(openFile(props.repoId, prefix)) },
      { label: "Show in Finder", run: () => runAction(revealFile(props.repoId, prefix)) },
      { label: "External Diff", disabled: true },
      { label: "Blame / Timeline…", disabled: true },
      { label: "History…", run: () => props.onOpenHistory?.(prefix) },
      { label: "Explain changes", run: () => props.onExplain?.({ kind: "path", path: prefix }, `Explain ${prefix}/`) },
      "divider",
      staged
        ? { label: `Unstage ${paths.length} File(s)`, run: async () => unstage(paths) }
        : { label: `Stage ${paths.length} File(s)`, run: async () => stage(paths) },
      { label: "Discard Changes…", run: () => runAction(discardChanges(props.repoId, paths)) },
      "divider",
      { label: "Ignore", run: () => runAction(ignore(props.repoId, [`${prefix}/`])) },
      { label: `Stash ${paths.length} File(s)…`, run: () => runAction(stashFiles(props.repoId, paths)) },
      { label: "Save as Patch…", run: () => runAction(saveAsPatch(props.repoId, paths)) },
      { label: "Copy Path", run: () => runAction(copyPath(prefix)) },
    ];
  };

  const openFileMenu = (e: MouseEvent, f: FileStatus, staged: boolean) => {
    e.preventDefault();
    setSelected({ path: f.path, staged });
    setMenu({ x: e.clientX, y: e.clientY, items: fileMenuItems(f, staged) });
  };
  const openFolderMenu = (e: MouseEvent, prefix: string, staged: boolean) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, items: folderMenuItems(prefix, staged) });
  };

  // ── Keyboard shortcuts (scoped to the focused file-lists column) ──────────
  // Bound on the column container (not globally) so ⌘C/⌘S/⌘D never hijack text
  // editing elsewhere. Acts on the currently selected file.
  const onColumnKeyDown = (e: KeyboardEvent) => {
    const s = selected();
    if (!s) return;
    // Enter (no modifier) stages the unstaged file (or unstages a staged one)
    // and advances to the next file in the list.
    if (e.key === "Enter" && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      stageOne(s.path, s.staged);
      return;
    }
    const mod = e.metaKey || e.ctrlKey;
    if (!mod) return;
    const k = e.key.toLowerCase();
    if (k === "s" && e.altKey) {
      e.preventDefault();
      s.staged ? unstage(allStagedPaths()) : stage(allUnstagedPaths());
    } else if (k === "s") {
      e.preventDefault();
      stageOne(s.path, s.staged);
    } else if (k === "d" && e.shiftKey) {
      e.preventDefault();
      runAction(discardChanges(props.repoId, [s.path]));
    } else if (k === "d") {
      e.preventDefault();
      externalDiff(s.path);
    } else if (k === "c") {
      e.preventDefault();
      runAction(copyPath(s.path));
    }
  };

  // ── Resizable Staged pane drag ─────────────────────────────────────────────
  let columnEl: HTMLDivElement | undefined;
  const startStagedDrag = (e: PointerEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = stagedHeight();
    const colH = columnEl?.clientHeight ?? window.innerHeight;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      // Drag up grows Staged / shrinks Unstaged; clamp so neither collapses.
      const next = startH + (startY - ev.clientY);
      setStagedHeight(Math.max(80, Math.min(next, colH - 120)));
    };
    const up = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  // ── Resizable file-lists column drag (horizontal) ──────────────────────────
  const startColumnDrag = (e: PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = changesColWidth();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      // Drag right grows the file list / shrinks the diff; clamp both sides.
      setChangesColWidth(Math.max(200, Math.min(startW + (ev.clientX - startX), window.innerWidth - 320)));
    };
    const up = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };

  // Toggle amend; turning it on prefills the last message if empty.
  const toggleAmend = () => {
    const on = !amend();
    setAmend(on);
    if (on && subject().trim() === "" && body().trim() === "" && recent().length > 0) {
      const last = recent()[0];
      const nl = last.indexOf("\n");
      setSubject(nl === -1 ? last : last.slice(0, nl));
      setBody(nl === -1 ? "" : last.slice(nl + 1).replace(/^\n/, ""));
    }
  };

  const stagedCount = () => wt()?.staged.length ?? 0;
  const canCommit = () => subject().trim().length > 0 && (stagedCount() > 0 || amend());

  const doCommit = () => {
    if (!canCommit()) return;
    invoke<string>("commit", { repo: props.repoId, message: message(), amend: amend(), sign: sign() })
      .then(() => {
        setSubject("");
        setBody("");
        setAmend(false);
        afterMutation();
      })
      .catch((e) => setErr(String(e)));
  };


  // Untracked files render in the Unstaged list (all stageable together).
  const untrackedAsFiles = (): FileStatus[] =>
    (wt()?.untracked ?? []).map((path) => ({ path, old_path: null, kind: "Untracked" as const }));
  const unstagedAll = (): FileStatus[] => [...(wt()?.unstaged ?? []), ...untrackedAsFiles()];
  const allUnstagedPaths = (): string[] => unstagedAll().map((f) => f.path);
  const allStagedPaths = (): string[] => (wt()?.staged ?? []).map((f) => f.path);

  // ── File list pieces ──────────────────────────────────────────────────────
  const subHeader = (title: string, count: number, action: { label: string; run: () => void; disabled?: boolean }) => (
    <div
      style={{
        display: "flex",
        "align-items": "center",
        gap: "8px",
        padding: `${ps(10)} 14px`,
        "flex-shrink": 0,
        background: "var(--sub)",
        "border-bottom": "1px solid var(--bd)",
      }}
    >
      <span style={{ "font-size": "12.5px", "font-weight": 600, color: "var(--tx)" }}>{title}</span>
      <span style={{ "font-size": "12.5px", color: "var(--tx3)" }}>{count}</span>
      <span style={{ flex: "1" }} />
      <button style={subBtn} disabled={action.disabled} onClick={action.run}>{action.label}</button>
    </div>
  );

  // `indent` (px) and `leaf` (basename) are set in tree mode; in list mode the
  // full path renders (with the rename arrow).
  const fileRow = (f: FileStatus, staged: boolean, indent = 0, leaf?: string) => (
    <div
      class="hov"
      onClick={() => setSelected({ path: f.path, staged })}
      onContextMenu={(e) => openFileMenu(e, f, staged)}
      style={{
        position: "relative",
        display: "flex",
        "align-items": "center",
        gap: "8px",
        padding: `${ps(8)} 14px`,
        "padding-left": `calc(${ps(8)} + ${6 + indent}px)`,
        cursor: "pointer",
        "font-size": "12.5px",
        color: "var(--tx2)",
        background: isSelected(f.path, staged) ? accentFill : "transparent",
        "box-shadow": isSelected(f.path, staged) ? "inset 2px 0 0 var(--accent)" : "none",
      }}
    >
      <Badge kind={f.kind} />
      <span style={{ flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap", "font-family": "ui-monospace, monospace" }} title={f.path}>
        <Show when={leaf} fallback={
          <>
            <Show when={f.old_path}>
              <span style={{ color: "var(--tx3)" }}>{f.old_path} → </span>
            </Show>
            {f.path}
          </>
        }>
          {leaf}
        </Show>
      </span>
      <button
        style={{ ...subBtn, padding: "2px 8px", "font-size": "11px" }}
        onClick={(e) => {
          e.stopPropagation();
          stageOne(f.path, staged);
        }}
      >
        {staged ? "Unstage" : "Stage"}
      </button>
    </div>
  );

  // Build directory + file nodes from flat paths for the tree layout. Each dir
  // carries its full (namespaced) key so it can be collapsed independently in
  // the staged vs unstaged buckets.
  type TreeNode =
    | { type: "dir"; depth: number; label: string; key: string }
    | { type: "file"; depth: number; file: FileStatus };
  const buildTree = (files: FileStatus[], ns: string): TreeNode[] => {
    const out: TreeNode[] = [];
    const seen = new Set<string>();
    for (const f of [...files].sort((a, b) => a.path.localeCompare(b.path))) {
      const parts = f.path.split("/");
      for (let i = 0; i < parts.length - 1; i++) {
        const key = `${ns}:${parts.slice(0, i + 1).join("/")}`;
        if (!seen.has(key)) {
          seen.add(key);
          out.push({ type: "dir", depth: i, label: parts[i], key });
        }
      }
      out.push({ type: "file", depth: parts.length - 1, file: f });
    }
    return out;
  };

  // Collapsed folder keys (namespaced by bucket). A node is hidden when any of
  // its ancestor folders is collapsed.
  const [collapsedDirs, setCollapsedDirs] = createSignal<Set<string>>(new Set());
  const toggleDir = (key: string) =>
    setCollapsedDirs((prev) => {
      const next = new Set(prev);
      next.has(key) ? next.delete(key) : next.add(key);
      return next;
    });

  // Keep only nodes whose every ancestor folder is expanded.
  const visibleNodes = (files: FileStatus[], staged: boolean): TreeNode[] => {
    const ns = staged ? "s" : "u";
    const col = collapsedDirs();
    return buildTree(files, ns).filter((node) => {
      const parts = node.type === "dir" ? node.key.slice(ns.length + 1).split("/") : node.file.path.split("/");
      // Check ancestor folders strictly above this node (so a collapsed folder
      // still renders its own row, only its descendants are hidden).
      for (let i = 0; i < parts.length - 1; i++) {
        if (col.has(`${ns}:${parts.slice(0, i + 1).join("/")}`)) return false;
      }
      return true;
    });
  };

  // Render a bucket as a flat list or a directory tree, per the user's setting.
  const fileList = (files: FileStatus[], staged: boolean) => (
    <Show
      when={changesLayout() === "tree"}
      fallback={<For each={files}>{(f) => fileRow(f, staged)}</For>}
    >
      <For each={visibleNodes(files, staged)}>
        {(node) =>
          node.type === "dir" ? (
            <div
              class="hov"
              onClick={() => toggleDir(node.key)}
              onContextMenu={(e) => openFolderMenu(e, node.key.slice((staged ? "s" : "u").length + 1), staged)}
              style={{ display: "flex", "align-items": "center", gap: "6px", padding: `${ps(6)} 14px`, "padding-left": `${14 + node.depth * 16}px`, "font-size": "12.5px", color: "var(--tx3)", "font-family": "ui-monospace, monospace", cursor: "pointer", "user-select": "none" }}
            >
              <span style={{ display: "inline-block", width: "10px" }}>{collapsedDirs().has(node.key) ? "▸" : "▾"}</span>
              {node.label}
            </div>
          ) : (
            fileRow(node.file, staged, node.depth * 16 + 16, node.file.path.split("/").pop())
          )
        }
      </For>
    </Show>
  );

  const composerField: JSX.CSSProperties = {
    width: "100%",
    "box-sizing": "border-box",
    background: "var(--input)",
    border: "1px solid var(--bd)",
    "border-radius": "7px",
    color: "var(--tx)",
    "font-family": "inherit",
    "font-size": "12.5px",
    padding: "9px 12px",
  };

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column", overflow: "hidden" }}>
      <div style={{ flex: "1", display: "flex", "min-height": "0", overflow: "hidden" }}>
        {/* File lists column (308px): a fixed Unstaged header, the Unstaged list
            (the only scrolling region), and a Staged pane pinned to the bottom
            that floats over the list. */}
        <div
          ref={columnEl}
          tabindex={0}
          onKeyDown={onColumnKeyDown}
          style={{ width: `${changesColWidth()}px`, "flex-shrink": 0, display: "flex", "flex-direction": "column", "min-height": "0", "border-right": "1px solid var(--bd)", background: "var(--panel)", outline: "none" }}
        >
          <Show when={err()}>
            <p style={{ color: "var(--error)", "font-size": "12px", padding: "8px 14px", margin: 0, "flex-shrink": 0 }}>{err()}</p>
          </Show>

          {subHeader("Unstaged", unstagedAll().length, {
            label: "Stage All",
            run: () => stage(allUnstagedPaths()),
            disabled: unstagedAll().length === 0,
          })}
          {/* Only the unstaged list scrolls. */}
          <div class="scroll-thin" style={{ flex: "1 1 0", "min-height": "60px", "overflow-y": "auto" }}>
            {fileList(unstagedAll(), false)}
          </div>

          {/* Drag handle: resize the Staged pane (grab strip / row-resize). */}
          <div
            onPointerDown={startStagedDrag}
            title="Drag to resize the Staged panel"
            style={{
              "flex-shrink": 0,
              height: "7px",
              cursor: "row-resize",
              background: "var(--sub)",
              "border-top": "1px solid var(--bd)",
              "box-shadow": "0 -8px 18px rgba(0, 0, 0, 0.22)",
              "touch-action": "none",
            }}
          />

          {/* Staged pane: pinned to the bottom, height user-adjustable via drag. */}
          <div
            style={{
              "flex-shrink": 0,
              display: "flex",
              "flex-direction": "column",
              "min-height": "0",
              height: `${stagedHeight()}px`,
            }}
          >
            {subHeader("Staged", stagedCount(), {
              label: "Unstage All",
              run: () => unstage(allStagedPaths()),
              disabled: stagedCount() === 0,
            })}
            <div class="scroll-thin" style={{ flex: "1 1 auto", "min-height": "0", "overflow-y": "auto" }}>
              {fileList(wt()?.staged ?? [], true)}
            </div>
          </div>
        </div>

        {/* Drag handle: resize the file-lists column (col-resize). */}
        <div
          onPointerDown={startColumnDrag}
          title="Drag to resize the file list"
          style={{ "flex-shrink": 0, width: "6px", cursor: "col-resize", "margin-left": "-3px", "z-index": 1, "touch-action": "none" }}
        />

        {/* Diff viewer (fills) */}
        <div style={{ flex: "1", "min-width": "0", overflow: "hidden" }}>
          <Show
            when={selectedSpec()}
            fallback={
              <div style={{ height: "100%", display: "flex", "align-items": "center", "justify-content": "center", color: "var(--tx3)", "font-size": "13px" }}>
                Select a file to view its diff.
              </div>
            }
          >
            <DiffView
              repoId={props.repoId}
              spec={selectedSpec()!}
              refreshKey={diffNonce()}
              hunkActionLabel={selected()!.staged ? "Unstage hunk" : "Stage hunk"}
              onHunkAction={selected()!.staged ? unstageHunk : stageHunk}
              onStageLines={selected()!.staged ? undefined : stageLines}
              onDiscardLines={selected()!.staged ? undefined : discardLines}
              onDiscardHunk={selected()!.staged ? undefined : discardHunk}
              onExplainHunk={
                props.onExplain
                  ? (path, patch) => props.onExplain!({ kind: "diff", diff: patch }, `Explain hunk`, path)
                  : undefined
              }
            />
          </Show>
        </div>
      </div>

      {/* Commit composer (pinned bottom, full width) */}
      <div style={{ "flex-shrink": 0, "border-top": "1px solid var(--bd)", background: "var(--panel)", padding: `${ps(14)} 16px`, display: "flex", "flex-direction": "column", gap: "8px" }}>
        <input
          style={composerField}
          placeholder="Commit subject"
          value={subject()}
          onInput={(e) => setSubject(e.currentTarget.value)}
        />
        <textarea
          style={{ ...composerField, resize: "none" }}
          rows={2}
          placeholder="Description"
          value={body()}
          onInput={(e) => setBody(e.currentTarget.value)}
        />
        <div style={{ display: "flex", "align-items": "center", gap: "12px", "flex-wrap": "wrap", "margin-top": "2px" }}>
          <label style={{ display: "flex", "align-items": "center", gap: "6px", "font-size": "12.5px", color: "var(--tx2)" }}>
            <input type="checkbox" checked={amend()} onChange={toggleAmend} />
            Amend last commit
          </label>
          <label style={{ display: "flex", "align-items": "center", gap: "6px", "font-size": "12.5px", color: "var(--tx2)" }}>
            <input type="checkbox" checked={sign()} onChange={() => setSign((s) => !s)} />
            Sign (-S)
          </label>
          <button
            style={{ ...subBtn, border: "1px solid var(--bd)", color: "var(--accent)", background: "transparent", cursor: aiBusy() ? "default" : "pointer" }}
            disabled={aiBusy()}
            onClick={generateMessage}
            title="Generate a commit message for the staged changes (AI)"
          >
            {aiBusy() ? "Generating…" : "✨ Generate"}
          </button>
          <Show when={aiBusy()}>
            <button style={subBtn} onClick={cancelGenerate}>Cancel</button>
          </Show>
          <Show when={recent().length > 0}>
            <select
              onChange={(e) => {
                if (e.currentTarget.value) {
                  const v = e.currentTarget.value;
                  const nl = v.indexOf("\n");
                  setSubject(nl === -1 ? v : v.slice(0, nl));
                  setBody(nl === -1 ? "" : v.slice(nl + 1).replace(/^\n/, ""));
                }
                e.currentTarget.selectedIndex = 0;
              }}
              style={{ "font-size": "12px", "max-width": "12rem", background: "var(--input)", color: "var(--tx)", border: "1px solid var(--bd)", "border-radius": "6px", padding: "4px 6px" }}
              title="Reuse a recent message"
            >
              <option value="">Recent…</option>
              <For each={recent()}>{(m) => <option value={m}>{m.split("\n")[0]}</option>}</For>
            </select>
          </Show>
          <span style={{ flex: "1" }} />
          <button
            style={{
              background: canCommit() ? "var(--accent)" : "var(--btn)",
              color: canCommit() ? "var(--on-accent-strong)" : "var(--tx3)",
              border: "none",
              "border-radius": "7px",
              padding: "8px 22px",
              "font-size": "12.5px",
              "font-weight": 600,
              cursor: canCommit() ? "pointer" : "not-allowed",
            }}
            disabled={!canCommit()}
            onClick={doCommit}
          >
            {amend() ? "Amend" : stagedCount() > 0 ? `Commit (${stagedCount()})` : "Commit"}
          </button>
        </div>
      </div>

      {/* File / folder right-click menu. */}
      <Show when={menu()}>
        <ContextMenu x={menu()!.x} y={menu()!.y} items={menu()!.items} onClose={() => setMenu(null)} />
      </Show>
    </div>
  );
};

export default ChangesView;
