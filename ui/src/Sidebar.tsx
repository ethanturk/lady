import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ForgeItem, RefInfo, RepoId, StashEntry } from "./commands";
import { IconChanges, IconCheck, IconChevron, IconCommits, IconBranch, IconMore, IconSearch } from "./icons";
import { sidebarWidth } from "./prefs";

export type PrimaryView = "changes" | "commits";

/** Which accordion panel is expanded (only one at a time). */
type Panel = "local" | "remote" | "stashes" | "prs" | "issues";

/** Scale a px padding by the global density step (--pad-scale). */
const ps = (px: number) => `calc(${px}px * var(--pad-scale))`;

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
  /** Check out `branch` (double-click on a branch/remote row). */
  onCheckout: (branch: string) => void;
  /** A keyboard shortcut fired on a focused branch row (⇧⌘B / ⇧⌘G / ⌫). */
  onBranchKey?: (branch: string, action: "new-branch" | "new-tag" | "delete") => void;
  /** Open the full Stashes management view. */
  onOpenStashes?: () => void;
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
  const isCurrent = (r: RefInfo) => r.kind === "Branch" && r.name === headBranch();

  // Accordion: a single open panel at a time (Local open by default).
  const [openPanel, setOpenPanel] = createSignal<Panel | null>("local");
  const toggle = (p: Panel) => setOpenPanel((cur) => (cur === p ? null : p));

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
    const panel = openPanel();
    void props.refreshNonce;
    if (!repo) return;
    if (panel === "stashes") {
      invoke<StashEntry[]>("stash_list", { repo }).then(setStashes).catch(() => setStashes([]));
    } else if (panel === "prs") {
      setPrLoading(true);
      setPrErr(null);
      invoke<ForgeItem[]>("list_pull_requests", { repo })
        .then(setPrs)
        .catch((e) => { setPrs([]); setPrErr(String(e)); })
        .finally(() => setPrLoading(false));
    } else if (panel === "issues") {
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

  const branchRow = (r: RefInfo, kind: "Branch" | "Remote" | "Tag") => (
    <div
      class="hov"
      tabindex={0}
      onContextMenu={(e) => {
        if (kind === "Branch") {
          e.preventDefault();
          props.onBranchMenu(r.name, { x: e.clientX, y: e.clientY });
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
        padding: `${ps(6)} 8px ${ps(6)} 26px`,
        "border-radius": "6px",
        "font-size": "13px",
        color: "var(--tx2)",
        "user-select": "none",
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
        {r.name}
      </span>
      <Show when={kind === "Branch"}>
        <button
          aria-label={`Actions for ${r.name}`}
          onClick={(e) => {
            const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
            props.onBranchMenu(r.name, { x: box.left, y: box.bottom });
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

  return (
    <div
      class="scroll-thin"
      style={{
        width: `${sidebarWidth()}px`,
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
        <AccordionPanel title="Local" count={branches().length} open={openPanel() === "local"} onToggle={() => toggle("local")}>
          <For each={branches()} fallback={<Note>No local branches.</Note>}>{(r) => branchRow(r, "Branch")}</For>
        </AccordionPanel>

        <AccordionPanel title="Remote" count={remotes().length} open={openPanel() === "remote"} onToggle={() => toggle("remote")}>
          <For each={remotes()} fallback={<Note>No remote branches.</Note>}>{(r) => branchRow(r, "Remote")}</For>
        </AccordionPanel>

        <AccordionPanel title="Stashes" count={stashes().length} open={openPanel() === "stashes"} onToggle={() => toggle("stashes")}>
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

        <AccordionPanel title="Pull Requests" count={prErr() ? undefined : prs().length} open={openPanel() === "prs"} onToggle={() => toggle("prs")}>
          <Show when={!prLoading()} fallback={<Note>Loading…</Note>}>
            <Show when={!prErr()} fallback={<Note>{prErr()}</Note>}>
              <For each={prs()} fallback={<Note>No open pull requests.</Note>}>{(it) => forgeRow(it)}</For>
            </Show>
          </Show>
        </AccordionPanel>

        <AccordionPanel title="Issues" count={issueErr() ? undefined : issues().length} open={openPanel() === "issues"} onToggle={() => toggle("issues")}>
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
