import { createEffect, createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { CustomCommand, OpenRepo, RecentRepo, RepoId, RepositoryFamily, Settings } from "./commands";

/** Last path segment, for a compact tab label. */
function baseName(path: string): string {
  const parts = path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

/** API handed up to App: open a known path, or pop the native repo picker. */
export interface RepoBarApi {
  open: (path: string) => void;
  pick: () => void;
}

const RepoBar: Component<{
  active: string | null;
  onActiveChange: (repo: OpenRepo | null) => void;
  /** Receives openers so other views (worktrees) and the toolbar can open repos. */
  apiRef?: (api: RepoBarApi) => void;
  /** Notified whenever the recent-repository list changes (for the launcher). */
  onRecents?: (recent: RecentRepo[]) => void;
}> = (props) => {
  const [opened, setOpened] = createSignal<OpenRepo[]>([]);
  const [recent, setRecent] = createSignal<RecentRepo[]>([]);
  // Preserved verbatim so saving recents never clobbers custom commands.
  const [customCommands, setCustomCommands] = createSignal<CustomCommand[]>([]);
  const [group, setGroup] = createSignal("");
  const [showClone, setShowClone] = createSignal(false);
  const [cloneUrl, setCloneUrl] = createSignal("");
  const [cloneDest, setCloneDest] = createSignal("");
  const [progress, setProgress] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);

  // Pop the OS folder picker and open the chosen directory as a repo.
  const pickAndOpen = async () => {
    setErr(null);
    try {
      const dir = await openDialog({ directory: true, multiple: false, title: "Open a Git repository" });
      if (typeof dir === "string") {
        openPath(dir, group().trim() || null);
        setPanelOpen(false);
        setGroup("");
      }
    } catch (e) {
      setErr(String(e));
    }
  };

  // Open tabs + focused tab, persisted across restarts (localStorage).
  const TABS_KEY = "lady-open-tabs";
  const [restored, setRestored] = createSignal(false);

  onMount(async () => {
    // Expose openers: a known-path opener (worktrees) + the native picker (toolbar).
    props.apiRef?.({ open: (p: string) => openPath(p, null), pick: pickAndOpen });
    try {
      const s = await invoke<Settings>("load_settings");
      setRecent(s.recent);
      setCustomCommands(s.custom_commands ?? []);
      props.onRecents?.(s.recent);
    } catch (e) {
      setErr(String(e));
    }
    // Reopen the tabs from last session and focus the previously-active one.
    try {
      const raw = localStorage.getItem(TABS_KEY);
      if (raw) {
        const data = JSON.parse(raw) as { tabs?: { path: string; group: string | null }[]; active?: string | null };
        for (const t of data.tabs ?? []) {
          await openPath(t.path, t.group ?? null); // skips silently if it no longer opens
        }
        const focus = opened().find((r) => r.path === data.active);
        if (focus) activate(focus);
      }
    } catch {
      /* corrupt entry — ignore */
    }
    setRestored(true);
  });

  // Persist the tab set + focus on any change (after the initial restore, so an
  // empty list during startup never clobbers the saved tabs).
  createEffect(() => {
    const list = opened();
    const activeId = props.active;
    if (!restored()) return;
    const data = {
      tabs: list.map((r) => ({ path: r.path, group: r.group })),
      active: list.find((r) => r.id === activeId)?.path ?? null,
    };
    localStorage.setItem(TABS_KEY, JSON.stringify(data));
  });

  const persistRecent = (next: RecentRepo[]) => {
    setRecent(next);
    props.onRecents?.(next);
    invoke("save_settings", {
      settings: { recent: next, custom_commands: customCommands() },
    }).catch((e) => setErr(String(e)));
  };

  const rememberRecent = (repo: OpenRepo) => {
    const next = [
      { path: repo.path, group: repo.group, family_id: repo.family_id, family_name: repo.family_name },
      ...recent().filter((r) => (r.family_id ?? r.path) !== repo.family_id && r.path !== repo.path),
    ].slice(0, 20);
    persistRecent(next);
  };

  const activate = (repo: OpenRepo) => props.onActiveChange(repo);

  /** Close a loaded repo's tab; if it was active, fall back to the last remaining
   * tab (or no repository when none are left). */
  const closeRepo = (repo: OpenRepo, e: MouseEvent) => {
    e.stopPropagation();
    const remaining = opened().filter((r) => r.id !== repo.id);
    setOpened(remaining);
    if (props.active === repo.id) {
      props.onActiveChange(remaining.length ? remaining[remaining.length - 1] : null);
    }
  };

  /** Open `p` (existing repo), de-duping by path; refresh its dirty flag. */
  const openPath = async (p: string, g: string | null) => {
    setErr(null);
    try {
      const id = await invoke<RepoId>("open_repo", { path: p });
      const family = await invoke<RepositoryFamily>("repository_family", { repo: id });
      const dirty = await invoke<boolean>("repo_dirty", { repo: id }).catch(() => false);
      const repo: OpenRepo = {
        path: family.worktrees.find((wt) => wt.selected)?.path ?? p,
        id,
        family_id: family.id,
        family_name: baseName(family.main.path),
        group: g,
        dirty,
      };
      setOpened((prev) => {
        const without = prev.filter((r) => r.family_id !== repo.family_id);
        return [...without, repo];
      });
      rememberRecent(repo);
      activate(repo);
    } catch (e) {
      setErr(String(e));
    }
  };

  const onClone = async () => {
    if (!cloneUrl() || !cloneDest()) return;
    setErr(null);
    setProgress("Cloning…");
    const unlisten = await listen<string>("clone-progress", (e) => setProgress(e.payload));
    try {
      await invoke<RepoId>("clone_repo", { url: cloneUrl(), dest: cloneDest() });
      setProgress("Done.");
      await openPath(cloneDest(), group() || null);
      setShowClone(false);
      setCloneUrl("");
      setCloneDest("");
    } catch (e) {
      setErr(String(e));
      setProgress("");
    } finally {
      unlisten();
    }
  };

  const [panelOpen, setPanelOpen] = createSignal(false);

  // Flat tab label: "group: name" when grouped, else the repo's base name.
  const tabLabel = (repo: OpenRepo) =>
    repo.group ? `${repo.group}: ${repo.family_name}` : repo.family_name;

  const tabStyle = (repo: OpenRepo) => {
    const on = props.active === repo.id;
    return {
      display: "flex",
      "align-items": "center",
      gap: "5px",
      height: "100%",
      padding: "0 14px",
      border: "none",
      "border-left": "1px solid var(--bd)",
      background: on ? "var(--tabact)" : "transparent",
      color: on ? "var(--tx)" : "var(--tx3)",
      "box-shadow": on ? "inset 0 2px 0 var(--accent)" : "none",
      "font-size": "12.5px",
      "white-space": "nowrap",
      cursor: "pointer",
    } as const;
  };

  const fieldStyle = {
    padding: "7px 10px",
    "font-size": "12.5px",
    background: "var(--input)",
    border: "1px solid var(--bd)",
    "border-radius": "7px",
    color: "var(--tx)",
  };

  return (
    <div style={{ "flex-shrink": 0, position: "relative" }}>
      {/* 34px repo tab strip */}
      <div
        class="scroll-thin"
        style={{
          display: "flex",
          "align-items": "stretch",
          height: "34px",
          background: "var(--tabs)",
          "border-bottom": "1px solid var(--bd)",
          "overflow-x": "auto",
        }}
      >
        <For each={opened()}>
          {(repo) => (
            <button
              class={props.active === repo.id ? undefined : "tab-inactive"}
              style={tabStyle(repo)}
              onClick={() => activate(repo)}
              title={repo.path}
            >
              <Show when={repo.dirty}>
                <span style={{ color: "var(--accent)" }}>●</span>
              </Show>
              {tabLabel(repo)}
              <span
                class="hov"
                role="button"
                aria-label={`Close ${tabLabel(repo)}`}
                title="Close repository"
                onClick={(e) => closeRepo(repo, e)}
                style={{
                  display: "flex",
                  "align-items": "center",
                  "justify-content": "center",
                  width: "16px",
                  height: "16px",
                  "margin-left": "2px",
                  "border-radius": "4px",
                  color: "var(--tx4)",
                  "font-size": "14px",
                  "line-height": "1",
                  cursor: "pointer",
                }}
              >
                ×
              </span>
            </button>
          )}
        </For>
        <button
          class="hov"
          aria-label="Add a repository"
          onClick={() => setPanelOpen((v) => !v)}
          style={{
            display: "flex",
            "align-items": "center",
            gap: "4px",
            padding: "0 14px",
            border: "none",
            "border-left": opened().length > 0 ? "1px solid var(--bd)" : "none",
            background: "transparent",
            color: "var(--tx3)",
            "font-size": "12.5px",
            "white-space": "nowrap",
            cursor: "pointer",
          }}
        >
          <span style={{ "font-size": "15px", "line-height": "1" }}>+</span> Add a Repo
        </button>
      </div>

      {/* Open / Clone / Recent popover (opened from the "+" button) */}
      <Show when={panelOpen()}>
        <div style={{ position: "fixed", inset: "0", "z-index": "30" }} onClick={() => setPanelOpen(false)} />
        <div
          style={{
            position: "absolute",
            top: "36px",
            right: "8px",
            "z-index": "31",
            width: "420px",
            "max-width": "92vw",
            background: "var(--pill)",
            border: "1px solid var(--bd)",
            "border-radius": "9px",
            padding: "12px",
            "box-shadow": "0 14px 38px rgba(0,0,0,0.45)",
            display: "flex",
            "flex-direction": "column",
            gap: "8px",
          }}
        >
          <input
            type="text"
            value={group()}
            onInput={(e) => setGroup(e.currentTarget.value)}
            placeholder="group (optional)"
            style={fieldStyle}
          />
          <div style={{ display: "flex", gap: "8px" }}>
            <button
              onClick={pickAndOpen}
              style={{ ...fieldStyle, flex: "1", cursor: "pointer", "font-weight": 600, background: "var(--accent)", color: "var(--on-accent-strong)", border: "none" }}
            >
              Browse for a folder…
            </button>
            <button onClick={() => setShowClone((v) => !v)} style={{ ...fieldStyle, cursor: "pointer" }}>Clone…</button>
          </div>

          <Show when={showClone()}>
            <div style={{ display: "flex", "flex-direction": "column", gap: "8px" }}>
              <input
                type="text"
                value={cloneUrl()}
                onInput={(e) => setCloneUrl(e.currentTarget.value)}
                placeholder="https://github.com/owner/repo.git"
                style={fieldStyle}
              />
              <input
                type="text"
                value={cloneDest()}
                onInput={(e) => setCloneDest(e.currentTarget.value)}
                placeholder="/path/to/dest"
                style={fieldStyle}
              />
              <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
                <button onClick={onClone} style={{ ...fieldStyle, cursor: "pointer" }}>Clone</button>
                <Show when={progress()}>
                  <span style={{ color: "var(--tx3)", "font-size": "12px" }}>{progress()}</span>
                </Show>
              </div>
            </div>
          </Show>

          <span style={{ color: "var(--tx4)", "font-size": "11px" }}>
            Recent repositories live in the Launch menu (top-left).
          </span>

          <Show when={err()}>
            <p style={{ color: "var(--error)", margin: 0, "font-size": "12px" }}>{err()}</p>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default RepoBar;
