import { createMemo, createSignal, For, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import type { RefInfo } from "./commands";
import { IconChanges, IconCheck, IconChevron, IconCommits, IconBranch, IconMore, IconSearch } from "./icons";
import { sidebarWidth } from "./prefs";

export type PrimaryView = "changes" | "commits";

/** Scale a px padding by the global density step (--pad-scale). */
const ps = (px: number) => `calc(${px}px * var(--pad-scale))`;

interface SidebarProps {
  repoName: string | null;
  /** Count shown on the Local Changes nav row. */
  changeCount: number;
  view: PrimaryView;
  onView: (v: PrimaryView) => void;
  refs: RefInfo[];
  /** Open the branch context menu for `branch` at the pointer location. */
  onBranchMenu: (branch: string, at: { x: number; y: number }) => void;
  /** Check out `branch` (double-click on a branch/remote row). */
  onCheckout: (branch: string) => void;
  /** A keyboard shortcut fired on a focused branch row (⇧⌘B / ⇧⌘G / ⌫). */
  onBranchKey?: (branch: string, action: "new-branch" | "new-tag" | "delete") => void;
}

const accentFill = "color-mix(in srgb, var(--accent) 18%, transparent)";

/** Collapsible tree section (Branches / Remotes / Tags …). */
const Section: Component<{ title: string; count: number; children: JSX.Element }> = (props) => {
  const [open, setOpen] = createSignal(true);
  return (
    <div>
      <button
        onClick={() => setOpen((v) => !v)}
        style={{
          display: "flex",
          "align-items": "center",
          gap: "6px",
          width: "100%",
          padding: "5px 6px",
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
        <IconChevron size={12} open={open()} style={{ color: "var(--tx4)" }} />
        <span style={{ flex: "1", "text-align": "left" }}>{props.title}</span>
        <span style={{ color: "var(--tx4)" }}>{props.count}</span>
      </button>
      <Show when={open()}>{props.children}</Show>
    </div>
  );
};

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
  const isCurrent = (r: RefInfo) => r.kind === "Branch" && r.name === headBranch();

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

      {/* Ref tree */}
      <div style={{ padding: "0 6px 16px", display: "flex", "flex-direction": "column", gap: "4px" }}>
        <Section title="Branches" count={branches().length}>
          <For each={branches()}>{(r) => branchRow(r, "Branch")}</For>
        </Section>
        <Show when={remotes().length > 0}>
          <Section title="Remotes" count={remotes().length}>
            <For each={remotes()}>{(r) => branchRow(r, "Remote")}</For>
          </Section>
        </Show>
        <Show when={tags().length > 0}>
          <Section title="Tags" count={tags().length}>
            <For each={tags()}>{(r) => branchRow(r, "Tag")}</For>
          </Section>
        </Show>
      </div>
    </div>
  );
};

export default Sidebar;
