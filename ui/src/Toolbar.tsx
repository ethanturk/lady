import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AheadBehind, RepoId } from "./commands";
import {
  IconBranch,
  IconFetch,
  IconLaunch,
  IconMenu,
  IconMore,
  IconPull,
  IconPush,
  IconSettings,
  IconStash,
} from "./icons";
import { isNarrow } from "./prefs";

/** One entry in the toolbar's "More" overflow menu (an advanced view). */
export interface OverflowItem {
  key: string;
  label: string;
  badge?: number;
}

interface ToolbarProps {
  repoId: RepoId | null;
  repoName: string | null;
  currentBranch: string | null;
  refreshNonce: number;
  /** Reload refs/status/graph after a sync or stash. */
  onChanged: () => void;
  /** Advanced views reachable from the "More" menu. */
  overflowItems: OverflowItem[];
  onOverflow: (key: string) => void;
  /** Open the command palette (Quick Launch). */
  onQuickLaunch: () => void;
  /** Toggle the off-canvas sidebar drawer (narrow layout hamburger). */
  onToggleSidebar?: () => void;
  /** Open the native repo picker (clicking the empty pill, when no repo is open). */
  onAddRepo: () => void;
  /** Open the branch context menu anchored to the Branch button. */
  onBranchMenu: (anchor: { x: number; y: number }) => void;
  /** Open the global Settings dialog (gear button, right group). */
  onSettings: () => void;
  /** Open the push confirmation dialog (required for all push operations). */
  onPush: () => void;
}

/** Vertical icon-over-label quick action (design toolbar left group). */
const QuickAction: Component<{
  icon: JSX.Element;
  label: string;
  title?: string;
  disabled?: boolean;
  onClick: (e: MouseEvent) => void;
}> = (props) => (
  <button
    class="hov"
    disabled={props.disabled}
    onClick={(e) => props.onClick(e)}
    style={{
      display: "flex",
      "flex-direction": "column",
      "align-items": "center",
      gap: "3px",
      padding: "5px 11px",
      border: "none",
      background: "transparent",
      "border-radius": "7px",
      color: "var(--tx2)",
      cursor: props.disabled ? "default" : "pointer",
      opacity: props.disabled ? 0.45 : 1,
      "font-size": "10px",
      "line-height": "1",
    }}
    title={props.title ?? props.label}
  >
    {props.icon}
    <span>{props.label}</span>
  </button>
);

/**
 * The top toolbar (58px): quick-action group (Quick Launch / Fetch / Pull /
 * Push / Stash), the centered repo·branch pill, and the right group (Branch
 * menu, More overflow, theme toggle). Fetch/Pull/Push/Stash logic lives here
 * (ported from the old SyncBar) and streams git's --progress into a status line.
 */
const Toolbar: Component<ToolbarProps> = (props) => {
  const [ab, setAb] = createSignal<AheadBehind | null>(null);
  const [busy, setBusy] = createSignal<string | null>(null);
  const [progress, setProgress] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);
  const [menuOpen, setMenuOpen] = createSignal(false);

  const unlisten: Array<() => void> = [];
  listen<string>("fetch-progress", (e) => setProgress(e.payload)).then((u) => unlisten.push(u));
  listen<string>("push-progress", (e) => setProgress(e.payload)).then((u) => unlisten.push(u));
  onCleanup(() => unlisten.forEach((u) => u()));

  const loadAheadBehind = () => {
    const id = props.repoId;
    if (!id) return setAb(null);
    invoke<AheadBehind | null>("ahead_behind", { repo: id }).then(setAb).catch(() => setAb(null));
  };

  createEffect(() => {
    props.repoId;
    props.refreshNonce;
    loadAheadBehind();
  });

  const run = async (label: string, cmd: string, args: Record<string, unknown>) => {
    const id = props.repoId;
    if (!id) return;
    setErr(null);
    setProgress("");
    setBusy(label);
    try {
      await invoke(cmd, { repo: id, ...args });
      props.onChanged();
      loadAheadBehind();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(null);
    }
  };

  const fetch = () => run("Fetching", "fetch", { remote: null });
  const pull = () => run("Pulling", "pull", { remote: null, branch: null });
  const stash = () =>
    run("Stashing", "stash_save", { message: null, includeUntracked: false });

  return (
    <div
      style={{
        // Grow by the top safe-area inset (notch / status bar) so content sits
        // below it on mobile; box-sizing keeps the usable bar at 58px.
        height: "calc(58px + env(safe-area-inset-top, 0px))",
        "flex-shrink": 0,
        display: "flex",
        "align-items": "center",
        gap: "14px",
        padding: "env(safe-area-inset-top, 0px) 16px 0",
        background: "var(--toolbar)",
        "border-bottom": "1px solid var(--bd)",
        position: "relative",
      }}
    >
      {/* Left quick-action group. On narrow the hamburger opens the sidebar
          drawer and the sync actions collapse into the "More" menu. */}
      <div style={{ display: "flex", "align-items": "center", gap: "2px" }}>
        <Show when={isNarrow()}>
          <QuickAction icon={<IconMenu />} label="Menu" disabled={!props.repoId} onClick={() => props.onToggleSidebar?.()} />
        </Show>
        <QuickAction icon={<IconLaunch />} label="Launch" onClick={() => props.onQuickLaunch()} />
        <Show when={!isNarrow()}>
          <QuickAction icon={<IconFetch />} label="Fetch" disabled={!props.repoId || !!busy()} onClick={fetch} />
          <QuickAction icon={<IconPull />} label="Pull" disabled={!props.repoId || !!busy()} onClick={pull} />
          <QuickAction icon={<IconPush />} label="Push" disabled={!props.repoId || !!busy()} onClick={() => props.onPush()} />
          <QuickAction icon={<IconStash />} label="Stash" disabled={!props.repoId || !!busy()} onClick={stash} />
        </Show>
      </div>

      <span style={{ flex: "1" }} />

      {/* Center repo · branch pill. With no repo open it's a button that opens
          the native picker (matches the "+ Add a Repo" flow). */}
      <div
        classList={{ hov: !props.repoId }}
        role={!props.repoId ? "button" : undefined}
        tabindex={!props.repoId ? 0 : undefined}
        onClick={() => !props.repoId && props.onAddRepo()}
        onKeyDown={(e) => !props.repoId && (e.key === "Enter" || e.key === " ") && props.onAddRepo()}
        style={{
          display: "flex",
          "flex-direction": "column",
          "align-items": "center",
          "min-width": isNarrow() ? "0" : "230px",
          padding: "6px 30px",
          background: "var(--pill)",
          border: "1px solid var(--bd)",
          "border-radius": "8px",
          cursor: props.repoId ? "default" : "pointer",
        }}
        title={busy() ? `${busy()}… ${progress()}` : props.repoId ? (err() ?? undefined) : "Open a repository"}
      >
        <span style={{ "font-size": "12.5px", "font-weight": 600, color: "var(--tx)" }}>
          {props.repoName ?? "Open a repository…"}
        </span>
        <span style={{ "font-size": "11px", color: "var(--tx3)", display: "flex", "align-items": "center", gap: "5px" }}>
          <IconBranch size={11} />
          <span>{props.currentBranch ?? "—"}</span>
          <Show when={busy()}>
            <span style={{ color: "var(--accent)" }}>· {busy()}…</span>
          </Show>
          <Show when={!busy() && ab()}>
            {(v) => (
              <span style={{ "font-family": "ui-monospace, monospace" }}>
                · ↑{v().ahead} ↓{v().behind}
              </span>
            )}
          </Show>
        </span>
      </div>

      <span style={{ flex: "1" }} />

      {/* Right group: Branch menu · More overflow · theme toggle */}
      <div style={{ display: "flex", "align-items": "center", gap: "6px" }}>
        <QuickAction
          icon={<IconBranch />}
          label="Branch"
          disabled={!props.repoId}
          onClick={(e) =>
            props.onBranchMenu({ x: (e.currentTarget as HTMLElement).getBoundingClientRect().left, y: 54 })
          }
        />
        <div style={{ position: "relative" }}>
          <QuickAction icon={<IconMore />} label="More" onClick={() => setMenuOpen((v) => !v)} />
          <Show when={menuOpen()}>
            {/* Backdrop closes the menu on any outside click. */}
            <div
              style={{ position: "fixed", inset: "0", "z-index": "40" }}
              onClick={() => setMenuOpen(false)}
            />
            <div
              class="scroll-thin"
              style={{
                position: "absolute",
                top: "52px",
                right: "0",
                "z-index": "41",
                "min-width": "210px",
                "max-height": "70vh",
                "overflow-y": "auto",
                background: "var(--pill)",
                border: "1px solid var(--bd)",
                "border-radius": "9px",
                padding: "5px",
                "box-shadow": "0 14px 38px rgba(0,0,0,0.45)",
              }}
              role="menu"
            >
              {/* On narrow the sync actions live here (they're hidden from the
                  toolbar's left group). */}
              <Show when={isNarrow()}>
                <For each={[
                  { label: "Fetch", run: fetch },
                  { label: "Pull", run: pull },
                  { label: "Push", run: () => props.onPush() },
                  { label: "Stash", run: stash },
                ]}>
                  {(item) => (
                    <button
                      class="hov"
                      role="menuitem"
                      disabled={!props.repoId || !!busy()}
                      onClick={() => {
                        setMenuOpen(false);
                        item.run();
                      }}
                      style={{
                        display: "flex",
                        "align-items": "center",
                        width: "100%",
                        gap: "9px",
                        padding: "7px 11px",
                        border: "none",
                        background: "transparent",
                        "border-radius": "6px",
                        color: "var(--tx)",
                        "font-size": "12.5px",
                        cursor: "pointer",
                        "text-align": "left",
                      }}
                    >
                      <span style={{ flex: "1" }}>{item.label}</span>
                    </button>
                  )}
                </For>
                <div style={{ height: "1px", background: "var(--bd)", margin: "5px 0" }} />
              </Show>
              <For each={props.overflowItems}>
                {(item) => (
                  <button
                    class="hov"
                    role="menuitem"
                    onClick={() => {
                      setMenuOpen(false);
                      props.onOverflow(item.key);
                    }}
                    style={{
                      display: "flex",
                      "align-items": "center",
                      width: "100%",
                      gap: "9px",
                      padding: "7px 11px",
                      border: "none",
                      background: "transparent",
                      "border-radius": "6px",
                      color: "var(--tx)",
                      "font-size": "12.5px",
                      cursor: "pointer",
                      "text-align": "left",
                    }}
                  >
                    <span style={{ flex: "1" }}>{item.label}</span>
                    <Show when={item.badge}>
                      <span
                        style={{
                          background: "var(--danger)",
                          color: "var(--on-accent)",
                          "border-radius": "9px",
                          padding: "0 6px",
                          "font-size": "10px",
                        }}
                      >
                        {item.badge}
                      </span>
                    </Show>
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
        <QuickAction
          icon={<IconSettings />}
          label="Settings"
          title="Settings (Cmd/Ctrl+,)"
          onClick={() => props.onSettings()}
        />
      </div>
    </div>
  );
};

export default Toolbar;
