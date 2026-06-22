import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AheadBehind, ForgeItem, RefInfo, RepoId, StashEntry } from "./commands";
import { IconChanges, IconCheck, IconChevron, IconCommits, IconBranch, IconMore, IconSearch } from "./icons";
import { sidebarWidth } from "./prefs";

export type PrimaryView = "changes" | "commits";

/** Which accordion panel is expanded (only one at a time). */
type Panel = "local" | "remote" | "tags" | "stashes" | "prs" | "issues";

/** Scale a px padding by the global density step (--pad-scale). */
const ps = (px: number) => `calc(${px}px * var(--pad-scale))`;
const treePad = (base: number, depth: number) => ps(base + depth * 14);

interface RefTreeNode {
  name: string;
  path: string;
  ref?: RefInfo;
  children: RefTreeNode[];
  childrenByName?: Map<string, RefTreeNode>;
}

const branchTree = (refs: RefInfo[]): RefTreeNode[] => {
  const root = new Map<string, RefTreeNode>();
  const ensure = (map: Map<string, RefTreeNode>, name: string, path: string) => {
    let node = map.get(name);
    if (!node) {
      node = { name, path, children: [] };
      map.set(name, node);
    }
    return node;
  };

  for (const ref of refs) {
    const parts = ref.name.split("/").filter(Boolean);
    let map = root;
    let path = "";
    for (const [index, part] of parts.entries()) {
      path = path ? `${path}/${part}` : part;
      const node = ensure(map, part, path);
      if (index === parts.length - 1) {
        node.ref = ref;
      }
      node.childrenByName ??= new Map<string, RefTreeNode>();
      map = node.childrenByName;
    }
  }

  const sortNodes = (map: Map<string, RefTreeNode>): RefTreeNode[] =>
    [...map.values()]
      .map((node) => {
        const childMap = node.childrenByName;
        node.children = childMap ? sortNodes(childMap) : [];
        delete node.childrenByName;
        return node;
      })
      .sort((a, b) => {
        const folderDelta = Number(b.children.length > 0) - Number(a.children.length > 0);
        return folderDelta || a.name.localeCompare(b.name);
      });

  return sortNodes(root);
};

const leafCount = (node: RefTreeNode): number =>
  (node.ref ? 1 : 0) + node.children.reduce((sum, child) => sum + leafCount(child), 0);

interface SidebarProps {
  repoId: RepoId | null;
  repoName: string | null;
  /** Count shown on the Local Changes nav row. */
  changeCount: number;
  /** Bump to reload stashes / PRs / issues after an external mutation. */
  refreshNonce?: number;
  view: PrimaryView;
  onView: (v: PrimaryView) => void;
  refs: RefInfo[];
  /** Open the branch context menu for `branch` at the pointer location. */
  onBranchMenu: (branch: string, at: { x: number; y: number }) => void;
  /** Open the tag context menu for `tag` at the pointer location. */
  onTagMenu?: (tag: string, at: { x: number; y: number }) => void;
  /** Check out `branch` (double-click on a branch/remote row). */
  onCheckout: (branch: string) => void;
  /** Single-click a ref row → show that branch/tag in All Commits (its tip). */
  onSelectRef?: (ref: RefInfo) => void;
  /** A keyboard shortcut fired on a focused branch row (⇧⌘B / ⇧⌘G / ⌫). */
  onBranchKey?: (branch: string, action: "new-branch" | "new-tag" | "delete") => void;
  /** Open the full Stashes management view. */
  onOpenStashes?: () => void;
  /** Fill the container width (used when hosted inside the mobile drawer). */
  fullWidth?: boolean;
}

const accentFill = "color-mix(in srgb, var(--accent) 18%, transparent)";

/** One accordion panel: a header (toggles open) over its body. */
const AccordionPanel: Component<{
  title: string;
  count?: number;
  open: boolean;
  onToggle: () => void;
  children: JSX.Element;
}> = (props) => (
  <div>
    <button
      onClick={() => props.onToggle()}
      aria-expanded={props.open}
      style={{
        display: "flex",
        "align-items": "center",
        gap: "6px",
        width: "100%",
        padding: "7px 6px",
        border: "none",
        background: "transparent",
        color: "var(--tx2)",
        "font-size": "11px",
        "font-weight": 600,
        "text-transform": "uppercase",
        "letter-spacing": "0.05em",
        cursor: "pointer",
      }}
    >
      <IconChevron size={12} open={props.open} style={{ color: "var(--tx4)" }} />
      <span style={{ flex: "1", "text-align": "left" }}>{props.title}</span>
      <Show when={props.count !== undefined}>
        <span style={{ color: "var(--tx4)" }}>{props.count}</span>
      </Show>
    </button>
    <Show when={props.open}>{props.children}</Show>
  </div>
);

/** Small muted note row inside a panel body (empty / error / loading state). */
const Note: Component<{ children: JSX.Element }> = (props) => (
  <div style={{ padding: "4px 10px 8px 26px", "font-size": "12px", color: "var(--tx3)" }}>{props.children}</div>
);

/**
 * Left sidebar (248px): repo header, the two primary nav items (Local Changes /
 * All Commits), a filter field, and the ref tree (Branches / Remotes / Tags).
 * Branch rows open the context menu via the ⋯ button or right-click (Phase 2).
 */
const Sidebar: Component<SidebarProps> = (props) => {
  const [filter, setFilter] = createSignal("");

  // The Head ref's name is the checked-out branch (set by the backend), so the
  // check mark lights up only the current branch — not every branch that happens
  // to share its tip commit.
  const headBranch = () => props.refs.find((r) => r.kind === "Head")?.name;
  const byKind = (kind: RefInfo["kind"]) =>
    props.refs
      .filter((r) => r.kind === kind && r.name.toLowerCase().includes(filter().toLowerCase()))
      .sort((a, b) => a.name.localeCompare(b.name));
  const branches = createMemo(() => byKind("Branch"));
  const remotes = createMemo(() => byKind("Remote"));
  const tags = createMemo(() => byKind("Tag"));
  const branchTreeNodes = createMemo(() => branchTree(branches()));
  const remoteTreeNodes = createMemo(() => branchTree(remotes()));
  const isCurrent = (r: RefInfo) => r.kind === "Branch" && r.name === headBranch();

  // The ref row last clicked (shown in All Commits), highlighted so the user
  // knows which branch they're viewing. Keyed `${kind}:${name}`.
  const [selectedRef, setSelectedRef] = createSignal<string | null>(null);
  const [closedFolders, setClosedFolders] = createSignal<Set<string>>(new Set());
  const folderKey = (kind: "Branch" | "Remote", path: string) => `${kind}:${path}`;
  const folderOpen = (kind: "Branch" | "Remote", path: string) => !closedFolders().has(folderKey(kind, path));
  const toggleFolder = (kind: "Branch" | "Remote", path: string) =>
    setClosedFolders((cur) => {
      const next = new Set(cur);
      const key = folderKey(kind, path);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  // Accordion: multiple panels may be open on tall screens; short screens keep
  // a single panel open at a time so the list never overflows awkwardly.
  const [openPanels, setOpenPanels] = createSignal<Set<Panel>>(new Set(["local"]));
  const [multiOpen, setMultiOpen] = createSignal(window.innerHeight >= 900);
  const onResize = () => setMultiOpen(window.innerHeight >= 900);
  window.addEventListener("resize", onResize);
  onCleanup(() => window.removeEventListener("resize", onResize));

  const isOpen = (p: Panel) => openPanels().has(p);
  const toggle = (p: Panel) =>
    setOpenPanels((cur) => {
      const next = new Set(cur);
      if (next.has(p)) {
        next.delete(p);
      } else if (multiOpen()) {
        next.add(p);
      } else {
        next.clear();
        next.add(p);
      }
      return next;
    });

  // Per-local-branch ahead/behind vs upstream (origin), keyed by branch name.
  const [aheadBehind, setAheadBehind] = createSignal<Record<string, AheadBehind>>({});
  createEffect(() => {
    const repo = props.repoId;
    void props.refreshNonce;
    if (!repo) return setAheadBehind({});
    invoke<Record<string, AheadBehind>>("branches_ahead_behind", { repo })
      .then(setAheadBehind)
      .catch(() => setAheadBehind({}));
  });

  // Lazily-loaded panel data (fetched when a panel is open / repo changes).
  const [stashes, setStashes] = createSignal<StashEntry[]>([]);
  const [prs, setPrs] = createSignal<ForgeItem[]>([]);
  const [issues, setIssues] = createSignal<ForgeItem[]>([]);
  const [prErr, setPrErr] = createSignal<string | null>(null);
  const [issueErr, setIssueErr] = createSignal<string | null>(null);
  const [prLoading, setPrLoading] = createSignal(false);
  const [issueLoading, setIssueLoading] = createSignal(false);

  createEffect(() => {
    const repo = props.repoId;
    void props.refreshNonce;
    if (!repo) return setStashes([]);
    invoke<StashEntry[]>("stash_list", { repo }).then(setStashes).catch(() => setStashes([]));
  });

  createEffect(() => {
    const repo = props.repoId;
    const open = openPanels();
    void props.refreshNonce;
    if (!repo) return;
    if (open.has("prs")) {
      setPrLoading(true);
      setPrErr(null);
      invoke<ForgeItem[]>("list_pull_requests", { repo })
        .then(setPrs)
        .catch((e) => { setPrs([]); setPrErr(String(e)); })
        .finally(() => setPrLoading(false));
    }
    if (open.has("issues")) {
      setIssueLoading(true);
      setIssueErr(null);
      invoke<ForgeItem[]>("list_issues", { repo })
        .then(setIssues)
        .catch((e) => { setIssues([]); setIssueErr(String(e)); })
        .finally(() => setIssueLoading(false));
    }
  });

  const openExternal = (url: string) => {
    invoke("open_url", { url }).catch(() => {});
  };

  // A pull-request / issue row (click opens it in the browser).
  const forgeRow = (it: ForgeItem) => (
    <div
      class="hov"
      onClick={() => openExternal(it.url)}
      title={it.title}
      style={{ display: "flex", "align-items": "baseline", gap: "6px", padding: `${ps(5)} 8px ${ps(5)} 26px`, "border-radius": "6px", cursor: "pointer", "font-size": "12.5px", color: "var(--tx2)", "user-select": "none" }}
    >
      <span style={{ color: "var(--tx4)", "font-family": "ui-monospace, monospace", "font-size": "11px", "flex-shrink": 0 }}>#{it.number}</span>
      <span style={{ flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>{it.title}</span>
      <Show when={it.draft}>
        <span style={{ color: "var(--tx4)", "font-size": "10px", "flex-shrink": 0 }}>draft</span>
      </Show>
    </div>
  );

  const navItem = (v: PrimaryView, icon: JSX.Element, label: string, trailing?: JSX.Element) => {
    const active = () => props.view === v;
    return (
      <button
        class="hov"
        onClick={() => props.onView(v)}
        aria-current={active() ? "page" : undefined}
        style={{
          position: "relative",
          display: "flex",
          "align-items": "center",
          gap: "9px",
          width: "100%",
          height: "32px",
          padding: "0 10px",
          border: "none",
          "border-radius": "7px",
          background: active() ? accentFill : "transparent",
          "box-shadow": active() ? "inset 2px 0 0 var(--accent)" : "none",
          color: active() ? "var(--tx)" : "var(--tx2)",
          "font-size": "12.5px",
          cursor: "pointer",
          "text-align": "left",
        }}
      >
        <span style={{ display: "flex", color: active() ? "var(--accent)" : "var(--tx3)" }}>{icon}</span>
        <span style={{ flex: "1" }}>{label}</span>
        {trailing}
      </button>
    );
  };

  const branchRow = (r: RefInfo, kind: "Branch" | "Remote" | "Tag", label = r.name, depth = 0) => {
    const rowKey = `${kind}:${r.name}`;
    const isShown = () => selectedRef() === rowKey;
    return (
    <div
      class="hov"
      tabindex={0}
      onClick={() => {
        setSelectedRef(rowKey);
        props.onSelectRef?.(r);
      }}
      onContextMenu={(e) => {
        if (kind === "Branch") {
          e.preventDefault();
          props.onBranchMenu(r.name, { x: e.clientX, y: e.clientY });
        } else if (kind === "Tag" && props.onTagMenu) {
          e.preventDefault();
          props.onTagMenu(r.name, { x: e.clientX, y: e.clientY });
        }
      }}
      onDblClick={() => {
        // Double-click a branch (or remote-tracking) row to check it out.
        if (kind !== "Tag" && !isCurrent(r)) props.onCheckout(r.name);
      }}
      onKeyDown={(e) => {
        // Keyboard parity with the branch menu (scoped to the focused row).
        if (kind !== "Branch") return;
        if (e.key === "Enter") {
          if (!isCurrent(r)) props.onCheckout(r.name);
        } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "b") {
          e.preventDefault();
          props.onBranchKey?.(r.name, "new-branch");
        } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "g") {
          e.preventDefault();
          props.onBranchKey?.(r.name, "new-tag");
        } else if ((e.key === "Backspace" || e.key === "Delete") && !isCurrent(r)) {
          e.preventDefault();
          props.onBranchKey?.(r.name, "delete");
        }
      }}
      title={kind === "Tag" ? r.name : `${r.name} — double-click to check out`}
      style={{
        display: "flex",
        "align-items": "center",
        gap: "6px",
        padding: `${ps(6)} 8px ${ps(6)} ${treePad(26, depth)}`,
        "border-radius": "6px",
        "font-size": "13px",
        color: "var(--tx2)",
        "user-select": "none",
        background: isShown() ? accentFill : "transparent",
        "box-shadow": isShown() ? "inset 2px 0 0 var(--accent)" : "none",
      }}
    >
      <Show
        when={isCurrent(r)}
        fallback={<IconBranch size={13} style={{ color: "var(--tx4)", "flex-shrink": 0 }} />}
      >
        <IconCheck size={13} style={{ color: "#46b06a", "flex-shrink": 0 }} />
      </Show>
      <span
        style={{
          flex: "1",
          overflow: "hidden",
          "text-overflow": "ellipsis",
          "white-space": "nowrap",
          "font-weight": isCurrent(r) ? 600 : 400,
          color: isCurrent(r) ? "var(--tx)" : "var(--tx2)",
        }}
        title={r.name}
      >
        {label}
      </span>
      {/* Outgoing (↑ local-only) / incoming (↓ remote-only) vs the paired branch. */}
      <Show when={kind !== "Tag" && aheadBehind()[r.name]}>
        {(() => {
          const ab = () => aheadBehind()[r.name];
          return (
            <Show when={ab().ahead > 0 || ab().behind > 0}>
              <span style={{ display: "flex", "align-items": "center", gap: "5px", "font-size": "11px", "font-family": "ui-monospace, monospace", color: "var(--tx3)", "flex-shrink": 0 }} title={`${ab().ahead} outgoing, ${ab().behind} incoming`}>
                <Show when={ab().ahead > 0}>
                  <span style={{ color: "var(--badge-a)" }}>↑{ab().ahead}</span>
                </Show>
                <Show when={ab().behind > 0}>
                  <span style={{ color: "var(--badge-r)" }}>↓{ab().behind}</span>
                </Show>
              </span>
            </Show>
          );
        })()}
      </Show>
      <Show when={kind === "Branch" || (kind === "Tag" && props.onTagMenu)}>
        <button
          aria-label={`Actions for ${r.name}`}
          onClick={(e) => {
            e.stopPropagation();
            const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
            if (kind === "Tag") props.onTagMenu?.(r.name, { x: box.left, y: box.bottom });
            else props.onBranchMenu(r.name, { x: box.left, y: box.bottom });
          }}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--tx4)",
            cursor: "pointer",
            display: "flex",
            padding: "0 2px",
          }}
        >
          <IconMore size={14} />
        </button>
      </Show>
    </div>
    );
  };

  const treeFolderRow = (node: RefTreeNode, kind: "Branch" | "Remote", depth: number) => {
    const open = () => folderOpen(kind, node.path);
    return (
      <button
        class="hov"
        onClick={() => toggleFolder(kind, node.path)}
        aria-expanded={open()}
        title={node.path}
        style={{
          display: "flex",
          "align-items": "center",
          gap: "6px",
          width: "100%",
          padding: `${ps(5)} 8px ${ps(5)} ${treePad(8, depth)}`,
          border: "none",
          "border-radius": "6px",
          background: "transparent",
          color: "var(--tx3)",
          "font-size": "12.5px",
          cursor: "pointer",
          "text-align": "left",
          "user-select": "none",
        }}
      >
        <IconChevron size={12} open={open()} style={{ color: "var(--tx4)", "flex-shrink": 0 }} />
        <span
          style={{
            flex: "1",
            overflow: "hidden",
            "text-overflow": "ellipsis",
            "white-space": "nowrap",
            "font-weight": 600,
          }}
        >
          {node.name}
        </span>
        <span style={{ color: "var(--tx4)", "font-size": "11px", "flex-shrink": 0 }}>{leafCount(node)}</span>
      </button>
    );
  };

  const treeNode = (node: RefTreeNode, kind: "Branch" | "Remote", depth = 0): JSX.Element => {
    if (node.children.length === 0) {
      return node.ref ? branchRow(node.ref, kind, node.name, depth) : null;
    }
    return (
      <>
        {treeFolderRow(node, kind, depth)}
        <Show when={folderOpen(kind, node.path)}>
          <Show when={node.ref}>{(ref) => branchRow(ref(), kind, node.name, depth + 1)}</Show>
          <For each={node.children}>{(child) => treeNode(child, kind, depth + 1)}</For>
        </Show>
      </>
    );
  };

  return (
    <div
      class="scroll-thin"
      style={{
        width: props.fullWidth ? "100%" : `${sidebarWidth()}px`,
        "flex-shrink": 0,
        "overflow-y": "auto",
        background: "var(--panel)",
        "border-right": "1px solid var(--bd)",
      }}
    >
      {/* Repo header */}
      <div style={{ padding: "14px 16px 8px", "font-size": "14px", "font-weight": 600, color: "var(--tx)" }}>
        {props.repoName ?? "No repository"}
      </div>

      {/* Primary nav */}
      <div style={{ padding: "0 8px", display: "flex", "flex-direction": "column", gap: "2px" }}>
        {navItem(
          "changes",
          <IconChanges size={16} />,
          "Local Changes",
          <Show when={props.changeCount > 0}>
            <span
              style={{
                background: "var(--hov)",
                "border-radius": "9px",
                padding: "1px 7px",
                "font-size": "11px",
                color: "var(--tx3)",
              }}
            >
              {props.changeCount}
            </span>
          </Show>,
        )}
        {navItem("commits", <IconCommits size={16} />, "All Commits")}
      </div>

      {/* Filter */}
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: "7px",
          margin: "14px",
          padding: "7px 10px",
          background: "var(--input)",
          border: "1px solid var(--bd)",
          "border-radius": "7px",
        }}
      >
        <IconSearch size={14} style={{ color: "var(--tx4)" }} />
        <input
          value={filter()}
          onInput={(e) => setFilter(e.currentTarget.value)}
          placeholder="Filter"
          style={{
            flex: "1",
            border: "none",
            background: "transparent",
            color: "var(--tx)",
            "font-size": "12.5px",
            outline: "none",
            padding: "0",
          }}
        />
      </div>

      {/* Accordion panels (one open at a time) */}
      <div style={{ padding: "0 6px 16px", display: "flex", "flex-direction": "column", gap: "2px" }}>
        <AccordionPanel title="Local" count={branches().length} open={isOpen("local")} onToggle={() => toggle("local")}>
          <For each={branchTreeNodes()} fallback={<Note>No local branches.</Note>}>{(node) => treeNode(node, "Branch")}</For>
        </AccordionPanel>

        <AccordionPanel title="Remote" count={remotes().length} open={isOpen("remote")} onToggle={() => toggle("remote")}>
          <For each={remoteTreeNodes()} fallback={<Note>No remote branches.</Note>}>{(node) => treeNode(node, "Remote")}</For>
        </AccordionPanel>

        <AccordionPanel title="Tags" count={tags().length} open={isOpen("tags")} onToggle={() => toggle("tags")}>
          <For each={tags()} fallback={<Note>No tags.</Note>}>{(r) => branchRow(r, "Tag")}</For>
        </AccordionPanel>

        <AccordionPanel title="Stashes" count={stashes().length} open={isOpen("stashes")} onToggle={() => toggle("stashes")}>
          <For each={stashes()} fallback={<Note>No stashes.</Note>}>
            {(s) => (
              <div
                class="hov"
                onClick={() => props.onOpenStashes?.()}
                title={s.message}
                style={{ display: "flex", "align-items": "baseline", gap: "6px", padding: `${ps(5)} 8px ${ps(5)} 26px`, "border-radius": "6px", cursor: "pointer", "font-size": "12.5px", color: "var(--tx2)", "user-select": "none" }}
              >
                <span style={{ color: "var(--accent-2)", "font-family": "ui-monospace, monospace", "font-size": "11px", "flex-shrink": 0 }}>{`{${s.index}}`}</span>
                <span style={{ flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>{s.message}</span>
              </div>
            )}
          </For>
          <Show when={stashes().length > 0}>
            <div class="hov" onClick={() => props.onOpenStashes?.()} style={{ padding: "4px 10px 8px 26px", "font-size": "11.5px", color: "var(--accent)", cursor: "pointer" }}>
              Manage stashes…
            </div>
          </Show>
        </AccordionPanel>

        <AccordionPanel title="Pull Requests" count={prErr() ? undefined : prs().length} open={isOpen("prs")} onToggle={() => toggle("prs")}>
          <Show when={!prLoading()} fallback={<Note>Loading…</Note>}>
            <Show when={!prErr()} fallback={<Note>{prErr()}</Note>}>
              <For each={prs()} fallback={<Note>No open pull requests.</Note>}>{(it) => forgeRow(it)}</For>
            </Show>
          </Show>
        </AccordionPanel>

        <AccordionPanel title="Issues" count={issueErr() ? undefined : issues().length} open={isOpen("issues")} onToggle={() => toggle("issues")}>
          <Show when={!issueLoading()} fallback={<Note>Loading…</Note>}>
            <Show when={!issueErr()} fallback={<Note>{issueErr()}</Note>}>
              <For each={issues()} fallback={<Note>No open issues.</Note>}>{(it) => forgeRow(it)}</For>
            </Show>
          </Show>
        </AccordionPanel>
      </div>
    </div>
  );
};

export default Sidebar;
