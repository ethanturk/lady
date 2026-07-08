import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AheadBehind, ChangeKind, DiffSpec, FileStatus, RepoId, WorkingTree } from "./commands";
import DiffView from "./DiffView";
import ContextMenu from "./ContextMenu";
import type { MenuEntry } from "./ContextMenu";
import { cancelAi, isConsentError, runAiStream } from "./ai";
import { changesColWidth, changesLayout, hideResizers, isNarrow, setChangesColWidth, setStagedHeight, stagedHeight } from "./prefs";
import { effectiveSign, repoSettings } from "./repoSettings";
import type { ActionResult } from "./branchActions";
import { copyPath, discardChanges, ignore, openFile, revealFile, saveAsPatch, stashFiles } from "./fileActions";
import { resolveFileSelection, sameSelection, selectionKey, type FileSelection as Selection } from "./fileSelection";

/** A 17px colored status badge for a file's change kind (design tokens). */
const KIND_BADGE: Record<ChangeKind, { label: string; bg: string; fg: string }> = {
  Added: { label: "A", bg: "var(--badge-a)", fg: "var(--on-badge)" },
  Modified: { label: "M", bg: "var(--badge-m)", fg: "var(--on-badge)" },
  Deleted: { label: "D", bg: "var(--badge-d)", fg: "var(--on-badge)" },
  Renamed: { label: "R", bg: "var(--badge-r)", fg: "var(--on-badge)" },
  // Untracked files are new files; show them as an Add (A), matching staged
  // adds. The staged/unstaged distinction is already conveyed by their list.
  Untracked: { label: "A", bg: "var(--badge-a)", fg: "var(--on-badge)" },
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

const splitPath = (path: string) => {
  const idx = path.lastIndexOf("/");
  return idx === -1 ? { dir: "", base: path } : { dir: path.slice(0, idx + 1), base: path.slice(idx + 1) };
};

/**
 * Wrap a string in a Unicode LTR isolate (U+2066 … U+2069) so it renders
 * left-to-right even inside a `direction: rtl` box. The rtl box is only used to
 * pin the truncation ellipsis to the left (showing the path's tail); without
 * this isolate the bidi algorithm reorders neutral boundary chars, e.g.
 * `.githooks/` would display as `/githooks.`.
 */
export const ltrIsolate = (s: string) => `\u2066${s}\u2069`;

const pathLabelBase: JSX.CSSProperties = {
  flex: "1",
  display: "flex",
  "align-items": "baseline",
  "min-width": "0",
  overflow: "hidden",
  "white-space": "nowrap",
  "font-family": "ui-monospace, monospace",
};

const pathPrefixStyle: JSX.CSSProperties = {
  "min-width": "0",
  overflow: "hidden",
  "text-overflow": "ellipsis",
  "white-space": "nowrap",
  direction: "rtl",
  "text-align": "left",
  color: "var(--tx3)",
};

const FilePathLabel: Component<{ path: string; oldPath?: string | null; leaf?: string }> = (props) => {
  const parts = () => splitPath(props.path);
  const title = () => (props.oldPath ? `${props.oldPath} -> ${props.path}` : props.path);
  return (
    <span style={pathLabelBase} title={title()}>
      <Show when={props.leaf} fallback={
        <>
          <Show when={props.oldPath}>
            <span style={{ ...pathPrefixStyle, flex: "0 1 35%", "max-width": "35%" }}>{ltrIsolate(props.oldPath!)}</span>
            <span style={{ color: "var(--tx3)", "flex-shrink": 0 }}>&nbsp;→&nbsp;</span>
          </Show>
          <Show when={parts().dir}>
            <span style={{ ...pathPrefixStyle, flex: "1 1 auto" }}>{ltrIsolate(parts().dir)}</span>
          </Show>
          <span style={{ "flex-shrink": 0, overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap", "padding-left": parts().dir ? "3px" : "0" }}>
            {parts().base}
          </span>
        </>
      }>
        {props.leaf}
      </Show>
    </span>
  );
};

interface ChangesViewProps {
  repoId: RepoId;
  /** Bump to force a status reload after an external mutation. */
  refreshNonce?: number;
  /** Called after a mutation here so sibling views (refs/graph) can reload. */
  onChanged?: () => void;
  /** Surface a file-action result (toast / error) in the app shell. */
  onResult?: (r: ActionResult | null) => void;
  /** Open the push confirmation dialog after committing (Shift+Commit). */
  onPush?: () => void;
  /** Open the Blame overlay focused on a path (file menu). */
  onOpenBlame?: (path: string) => void;
  /** Open the File-History overlay focused on a path (file menu). */
  onOpenHistory?: (path: string) => void;
  /** Open the AI "Explain changes" overlay for an ai_explain target. */
  onExplain?: (target: Record<string, unknown>, title: string, subtitle?: string) => void;
  /** Hand a pre-commit/hook failure (or `null` to clear) up to the app shell,
   * which surfaces it in the centered hook-error dialog. */
  onHookError?: (text: string | null) => void;
}

/**
 * Heuristic: does a failed-commit message look like git-hook output (pre-commit,
 * husky, …) rather than a one-line git error? Hook reports are multi-line and/or
 * carry the framework's status markers. These get routed to the dedicated
 * dialog instead of the cramped inline error line.
 */
const looksLikeHookError = (msg: string): boolean =>
  /hook id:|pre-commit|husky/i.test(msg) ||
  (msg.includes("\n") && /\b(Passed|Failed|Skipped)\b/.test(msg));

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
  const [selectedFiles, setSelectedFiles] = createSignal<Selection[]>([]);
  const [selectionAnchor, setSelectionAnchor] = createSignal<Selection | null>(null);
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
  // Commit + pre-commit hooks run on a backend thread; `committing` disables
  // the button and `hookLines` collects the streamed hook output for live
  // feedback while it runs.
  const [committing, setCommitting] = createSignal(false);
  const [hookLines, setHookLines] = createSignal<string[]>([]);

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
      // A user-initiated cancel is not an error — leave whatever streamed so far.
      if (!/cancelled|canceled/i.test(msg)) {
        setErr(isConsentError(msg) ? "AI consent required — enable the provider and grant consent in Settings." : msg);
      }
    } finally {
      setAiBusy(false);
      setAiReq(null);
    }
  };

  const cancelGenerate = () => {
    const id = aiReq();
    if (id) cancelAi(id).catch(() => {});
  };

  const wtSignature = (wt: WorkingTree) =>
    [
      ...wt.staged.map((f) => `${f.path}\0${f.kind}`),
      ...wt.unstaged.map((f) => `${f.path}\0${f.kind}`),
      ...wt.untracked.map((path) => `${path}\0Untracked`),
    ]
      .sort()
      .join("\n");

  const reload = (clearErr = false) => {
    if (clearErr) setErr(null);
    invoke<WorkingTree>("status", { repo: props.repoId })
      .then((newWt) => {
        const prev = wt();
        if (prev && wtSignature(prev) !== wtSignature(newWt)) setDiffNonce((n) => n + 1);
        setWt(newWt);
      })
      .catch((e) => setErr(String(e)));
    invoke<string[]>("recent_messages", { repo: props.repoId, limit: 10 }).then(setRecent).catch(() => setRecent([]));
  };

  const selectedSpec = (): DiffSpec | null => {
    const s = selected();
    if (!s) return null;
    return { kind: s.staged ? "IndexVsHead" : "WorkingVsIndex", value: s.path };
  };
  const replaceSelection = (entries: Selection[], primary: Selection | null, anchor: Selection | null = primary) => {
    setSelectedFiles(entries);
    setSelected(primary);
    setSelectionAnchor(anchor);
  };
  const isSelected = (path: string, staged: boolean) =>
    selectedFiles().some((sel) => sel.path === path && sel.staged === staged);
  const isPrimary = (path: string, staged: boolean) =>
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

  createEffect(() => {
    const available = new Set([
      ...unstagedAll().map((f) => selectionKey({ path: f.path, staged: false })),
      ...(wt()?.staged ?? []).map((f) => selectionKey({ path: f.path, staged: true })),
    ]);
    const current = selectedFiles();
    const next = current.filter((sel) => available.has(selectionKey(sel)));
    if (next.length !== current.length || next.some((sel, i) => !sameSelection(sel, current[i]))) {
      setSelectedFiles(next);
    }
    const primary = selected();
    if (primary && !available.has(selectionKey(primary))) {
      setSelected(next[next.length - 1] ?? null);
    }
    const anchor = selectionAnchor();
    if (anchor && !available.has(selectionKey(anchor))) {
      setSelectionAnchor(null);
    }
  });

  const afterMutation = (clearErr = true) => {
    reload(clearErr);
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
    replaceSelection(
      nextFile ? [{ path: nextFile.path, staged: fromStaged }] : [],
      nextFile ? { path: nextFile.path, staged: fromStaged } : null,
    );
  };

  const toggleStagePaths = (paths: string[], fromStaged: boolean, focus?: Selection | null) => {
    if (paths.length === 0) return;
    if (focus && paths.includes(focus.path)) advanceSelection(focus.path, fromStaged);
    fromStaged ? unstage(paths) : stage(paths);
  };

  // Stage/unstage a single file, advancing the selection to the next one.
  const stageOne = (path: string, fromStaged: boolean) => {
    toggleStagePaths([path], fromStaged, { path, staged: fromStaged });
  };

  const stage = (paths: string[]) => {
    if (paths.length === 0) return;
    invoke("stage_paths", { repo: props.repoId, paths }).then(() => afterMutation()).catch((e) => setErr(String(e)));
  };
  const unstage = (paths: string[]) => {
    if (paths.length === 0) return;
    invoke("unstage_paths", { repo: props.repoId, paths }).then(() => afterMutation()).catch((e) => setErr(String(e)));
  };
  const stageHunk = (path: string, hunk: number) => {
    invoke("stage_hunks", { repo: props.repoId, path, hunks: [hunk] }).then(() => afterMutation()).catch((e) => setErr(String(e)));
  };
  const unstageHunk = (path: string, hunk: number) => {
    invoke("unstage_hunks", { repo: props.repoId, path, hunks: [hunk] }).then(() => afterMutation()).catch((e) => setErr(String(e)));
  };
  const stageLines = (path: string, hunk: number, lines: number[]) => {
    invoke("stage_lines", { repo: props.repoId, path, hunk, lines }).then(() => afterMutation()).catch((e) => setErr(String(e)));
  };
  const discardLines = (path: string, hunk: number, lines: number[]) => {
    invoke("discard_lines", { repo: props.repoId, path, hunk, lines }).then(() => afterMutation()).catch((e) => setErr(String(e)));
  };
  const discardHunk = (path: string, hunk: number) => {
    invoke("discard_hunks", { repo: props.repoId, path, hunks: [hunk] }).then(() => afterMutation()).catch((e) => setErr(String(e)));
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
    const picked = actionPathsForFile(path, staged);
    const all = staged ? allStagedPaths() : allUnstagedPaths();
    const multi = picked.length > 1;
    return [
      { label: "Open", run: () => runAction(openFile(props.repoId, path)) },
      { label: "Show in Finder", run: () => runAction(revealFile(props.repoId, path)) },
      { label: "External Diff", shortcut: "⌘D", run: () => externalDiff(path) },
      { label: "Blame / Timeline…", run: () => props.onOpenBlame?.(path) },
      { label: "History…", run: () => props.onOpenHistory?.(path) },
      { label: "Explain changes", run: () => props.onExplain?.({ kind: "path", path }, `Explain ${path}`) },
      "divider",
      staged
        ? { label: multi ? `Unstage ${picked.length} Files` : "Unstage", shortcut: "⌘S", run: async () => toggleStagePaths(picked, true, { path, staged: true }) }
        : { label: multi ? `Stage ${picked.length} Files` : "Stage", shortcut: "⌘S", run: async () => toggleStagePaths(picked, false, { path, staged: false }) },
      { label: multi ? `Discard ${picked.length} Files…` : "Discard Changes…", shortcut: "⇧⌘D", disabled: !multi && f.kind === "Untracked", run: () => runAction(discardChanges(props.repoId, picked)) },
      { label: "Stage All", shortcut: "⌥⌘S", run: async () => stage(all) },
      "divider",
      { label: multi ? `Ignore ${picked.length} Files` : "Ignore", run: () => runAction(ignore(props.repoId, picked)) },
      { label: multi ? `Stash ${picked.length} Files…` : "Stash 1 File…", run: () => runAction(stashFiles(props.repoId, picked)) },
      { label: multi ? `Save ${picked.length} Files as Patch…` : "Save as Patch…", run: () => runAction(saveAsPatch(props.repoId, picked)) },
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
    if (!isSelected(f.path, staged)) {
      replaceSelection([{ path: f.path, staged }], { path: f.path, staged });
    }
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
    const paths = actionPathsForFile(s.path, s.staged);
    // Enter (no modifier) stages the unstaged file (or unstages a staged one)
    // and advances to the next file in the list.
    if (e.key === "Enter" && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      toggleStagePaths(paths, s.staged, s);
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
      toggleStagePaths(paths, s.staged, s);
    } else if (k === "d" && e.shiftKey) {
      e.preventDefault();
      runAction(discardChanges(props.repoId, paths));
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

  // Holding Shift turns "Commit" into "Commit + Push" (commit, then push the
  // current branch to its remote).
  const [shiftHeld, setShiftHeld] = createSignal(false);
  onMount(() => {
    const down = (e: KeyboardEvent) => { if (e.key === "Shift") setShiftHeld(true); };
    const up = (e: KeyboardEvent) => { if (e.key === "Shift") setShiftHeld(false); };
    const blur = () => setShiftHeld(false);
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", blur);
    onCleanup(() => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    });
  });

  const doCommit = async (push: boolean) => {
    if (!canCommit() || committing()) return;
    setErr(null);
    setHookLines([]);
    setCommitting(true);
    // Each line the backend relays from the pre-commit hooks' stdout.
    const unlisten = await listen<string>("commit-progress", (e) => {
      setHookLines((prev) => [...prev, e.payload]);
    });
    try {
      await invoke<string>("commit", { repo: props.repoId, message: message(), amend: amend(), sign: sign() });
      // A clean commit clears any lingering hook failure from a prior attempt.
      props.onHookError?.(null);
      setSubject("");
      setBody("");
      setAmend(false);
      if (push) {
        // Hand off to the app-level push dialog so the user can confirm the
        // remote and opt into a force push before the network request.
        props.onPush?.();
      }
      afterMutation();
    } catch (e) {
      const msg = String(e);
      // Hook failures (verbose, multi-line) go to the centered dialog so they
      // don't overflow the Changes column; plain git errors stay inline.
      if (looksLikeHookError(msg) && props.onHookError) {
        props.onHookError(msg);
      } else {
        setErr(msg);
      }
    } finally {
      unlisten();
      setCommitting(false);
      setHookLines([]);
    }
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
      onClick={(e) => handleFileClick(f.path, staged, e)}
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
        "box-shadow": isPrimary(f.path, staged) ? "inset 2px 0 0 var(--accent)" : "none",
      }}
    >
      <Badge kind={f.kind} />
      <FilePathLabel path={f.path} oldPath={f.old_path} leaf={leaf} />
      <button
        style={{ ...subBtn, padding: "2px 8px", "font-size": "11px" }}
        onClick={(e) => {
          e.stopPropagation();
          toggleStagePaths(actionPathsForFile(f.path, staged), staged, { path: f.path, staged });
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

  const visibleFileSelections = (files: FileStatus[], staged: boolean): Selection[] =>
    changesLayout() === "tree"
      ? visibleNodes(files, staged)
          .filter((node): node is Extract<TreeNode, { type: "file" }> => node.type === "file")
          .map((node) => ({ path: node.file.path, staged }))
      : files.map((f) => ({ path: f.path, staged }));

  const selectableFiles = (): Selection[] => [
    ...visibleFileSelections(unstagedAll(), false),
    ...visibleFileSelections(wt()?.staged ?? [], true),
  ];

  const selectedPathsForBucket = (staged: boolean): string[] =>
    selectedFiles()
      .filter((sel) => sel.staged === staged)
      .map((sel) => sel.path);

  const actionPathsForFile = (path: string, staged: boolean): string[] => {
    const bucket = selectedPathsForBucket(staged);
    return isSelected(path, staged) && bucket.length > 1 ? bucket : [path];
  };

  const handleFileClick = (path: string, staged: boolean, e: MouseEvent) => {
    const next = resolveFileSelection(selectableFiles(), selectedFiles(), selectionAnchor(), { path, staged }, {
      meta: e.metaKey || e.ctrlKey,
      shift: e.shiftKey,
    });
    replaceSelection(next.selected, next.primary, next.anchor);
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
      <div style={{ flex: "1", display: "flex", "flex-direction": isNarrow() ? "column" : "row", "min-height": "0", overflow: "hidden" }}>
        {/* File lists column (308px wide; on narrow it stacks on top, capped at
            45% height). Fixed Unstaged header, the Unstaged list (the only
            scrolling region), and a Staged pane pinned to the bottom. */}
        <div
          ref={columnEl}
          tabindex={0}
          onKeyDown={onColumnKeyDown}
          style={
            isNarrow()
              ? { width: "100%", "flex-shrink": 1, "max-height": "45%", display: "flex", "flex-direction": "column", "min-height": "0", "border-bottom": "1px solid var(--bd)", background: "var(--panel)", outline: "none" }
              : { width: `${changesColWidth()}px`, "flex-shrink": 0, display: "flex", "flex-direction": "column", "min-height": "0", "border-right": "1px solid var(--bd)", background: "var(--panel)", outline: "none" }
          }
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

          {/* Drag handle: resize the Staged pane (grab strip / row-resize).
              Hidden on touch/narrow, where the staged pane flexes instead. */}
          <Show when={!hideResizers()}>
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
          </Show>

          {/* Staged pane: pinned to the bottom. On wide its height is drag-set;
              on narrow it flexes to share the available column height. */}
          <div
            style={{
              "flex-shrink": isNarrow() ? 1 : 0,
              display: "flex",
              "flex-direction": "column",
              "min-height": "0",
              height: isNarrow() ? "auto" : `${stagedHeight()}px`,
              flex: isNarrow() ? "1 1 0" : undefined,
              "border-top": isNarrow() ? "1px solid var(--bd)" : undefined,
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

        {/* Drag handle: resize the file-lists column (col-resize). Hidden on
            touch/narrow, where the lists stack above the diff. */}
        <Show when={!hideResizers()}>
          <div
            onPointerDown={startColumnDrag}
            title="Drag to resize the file list"
            style={{ "flex-shrink": 0, width: "6px", cursor: "col-resize", "margin-left": "-3px", "z-index": 1, "touch-action": "none" }}
          />
        </Show>

        {/* Diff viewer (fills) */}
        <div style={{ flex: "1", "min-width": "0", "min-height": "0", overflow: "hidden" }}>
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
      <div style={{ "flex-shrink": 0, "border-top": "1px solid var(--bd)", background: "var(--panel)", padding: `${ps(14)} 16px`, "padding-bottom": `calc(${ps(14)} + env(safe-area-inset-bottom, 0px))`, display: "flex", "flex-direction": "column", gap: "8px" }}>
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
        <Show when={committing()}>
          <div style={{ display: "flex", "align-items": "center", gap: "8px", "font-size": "12px", color: "var(--tx2)", "min-height": "16px" }}>
            <span class="spin" style={{ display: "inline-block", width: "12px", height: "12px", border: "2px solid var(--bd)", "border-top-color": "var(--accent)", "border-radius": "50%" }} />
            <span style={{ "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis" }}>
              {hookLines().length > 0 ? hookLines()[hookLines().length - 1] : "Running pre-commit hooks…"}
            </span>
          </div>
        </Show>
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
              background: canCommit() && !committing() ? "var(--accent)" : "var(--btn)",
              color: canCommit() && !committing() ? "var(--on-accent-strong)" : "var(--tx3)",
              border: "none",
              "border-radius": "7px",
              padding: "8px 22px",
              "font-size": "12.5px",
              "font-weight": 600,
              cursor: canCommit() && !committing() ? "pointer" : "not-allowed",
            }}
            disabled={!canCommit() || committing()}
            onClick={() => doCommit(shiftHeld())}
            title={shiftHeld() ? "Commit and push to the remote" : "Hold Shift to commit and push"}
          >
            {(() => {
              if (committing()) return "Committing…";
              const verb = amend() ? "Amend" : stagedCount() > 0 ? `Commit (${stagedCount()})` : "Commit";
              return shiftHeld() ? `${verb} + Push` : verb;
            })()}
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
