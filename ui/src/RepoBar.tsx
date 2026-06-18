import { createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CustomCommand, OpenRepo, RecentRepo, RepoId, Settings } from "./commands";

/** Last path segment, for a compact tab label. */
function baseName(path: string): string {
  const parts = path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

const RepoBar: Component<{
  active: string | null;
  onActiveChange: (repo: OpenRepo | null) => void;
  /** Receives an opener so other views (e.g. worktrees) can open a path as a tab. */
  apiRef?: (open: (path: string) => void) => void;
}> = (props) => {
  const [opened, setOpened] = createSignal<OpenRepo[]>([]);
  const [recent, setRecent] = createSignal<RecentRepo[]>([]);
  // Preserved verbatim so saving recents never clobbers custom commands.
  const [customCommands, setCustomCommands] = createSignal<CustomCommand[]>([]);
  const [path, setPath] = createSignal("");
  const [group, setGroup] = createSignal("");
  const [showClone, setShowClone] = createSignal(false);
  const [cloneUrl, setCloneUrl] = createSignal("");
  const [cloneDest, setCloneDest] = createSignal("");
  const [progress, setProgress] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);

  onMount(async () => {
    // Expose an opener so worktrees can open a path as a repo tab.
    props.apiRef?.((p: string) => openPath(p, null));
    try {
      const s = await invoke<Settings>("load_settings");
      setRecent(s.recent);
      setCustomCommands(s.custom_commands ?? []);
    } catch (e) {
      setErr(String(e));
    }
  });

  const persistRecent = (next: RecentRepo[]) => {
    setRecent(next);
    invoke("save_settings", {
      settings: { recent: next, custom_commands: customCommands() },
    }).catch((e) => setErr(String(e)));
  };

  const rememberRecent = (p: string, g: string | null) => {
    const next = [{ path: p, group: g }, ...recent().filter((r) => r.path !== p)].slice(0, 20);
    persistRecent(next);
  };

  const activate = (repo: OpenRepo) => props.onActiveChange(repo);

  /** Open `p` (existing repo), de-duping by path; refresh its dirty flag. */
  const openPath = async (p: string, g: string | null) => {
    setErr(null);
    try {
      const id = await invoke<RepoId>("open_repo", { path: p });
      const dirty = await invoke<boolean>("repo_dirty", { repo: id }).catch(() => false);
      const repo: OpenRepo = { path: p, id, group: g, dirty };
      setOpened((prev) => {
        const without = prev.filter((r) => r.path !== p);
        return [...without, repo];
      });
      rememberRecent(p, g);
      activate(repo);
    } catch (e) {
      setErr(String(e));
    }
  };

  const onOpen = () => {
    if (!path()) return;
    openPath(path(), group() || null);
    setPath("");
    setGroup("");
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

  // Distinct groups in first-seen order; ungrouped repos collected under null.
  const groups = () => {
    const seen: (string | null)[] = [];
    for (const r of opened()) {
      const g = r.group ?? null;
      if (!seen.includes(g)) seen.push(g);
    }
    return seen;
  };

  const tabStyle = (repo: OpenRepo) => ({
    padding: "0.25rem 0.6rem",
    cursor: "pointer",
    border: "1px solid var(--border)",
    "border-radius": "4px",
    "font-size": "0.8rem",
    background: props.active === repo.id ? "var(--accent)" : "var(--surface-2)",
    color: props.active === repo.id ? "var(--on-accent)" : "var(--fg)",
    "white-space": "nowrap",
  });

  return (
    <div style={{ "flex-shrink": 0, "border-bottom": "1px solid var(--border)", padding: "0.5rem 1rem" }}>
      <div style={{ display: "flex", gap: "0.5rem", "flex-wrap": "wrap" }}>
        <input
          type="text"
          value={path()}
          onInput={(e) => setPath(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onOpen();
          }}
          placeholder="/path/to/repo"
          style={{ flex: "1", "min-width": "12rem", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
        />
        <input
          type="text"
          value={group()}
          onInput={(e) => setGroup(e.currentTarget.value)}
          placeholder="group (optional)"
          style={{ width: "9rem", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
        />
        <button onClick={onOpen} style={{ padding: "0.3rem 0.8rem" }}>
          Open / Add
        </button>
        <button onClick={() => setShowClone((v) => !v)} style={{ padding: "0.3rem 0.8rem" }}>
          Clone…
        </button>
      </div>

      <Show when={showClone()}>
        <div style={{ display: "flex", gap: "0.5rem", "margin-top": "0.4rem", "flex-wrap": "wrap" }}>
          <input
            type="text"
            value={cloneUrl()}
            onInput={(e) => setCloneUrl(e.currentTarget.value)}
            placeholder="https://github.com/owner/repo.git"
            style={{ flex: "1", "min-width": "14rem", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
          />
          <input
            type="text"
            value={cloneDest()}
            onInput={(e) => setCloneDest(e.currentTarget.value)}
            placeholder="/path/to/dest"
            style={{ flex: "1", "min-width": "10rem", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
          />
          <button onClick={onClone} style={{ padding: "0.3rem 0.8rem" }}>
            Clone
          </button>
          <Show when={progress()}>
            <span style={{ color: "var(--fg-muted)", "font-size": "0.8rem", "align-self": "center" }}>
              {progress()}
            </span>
          </Show>
        </div>
      </Show>

      <Show when={err()}>
        <p style={{ color: "var(--error)", margin: "0.3rem 0 0", "font-size": "0.8rem" }}>{err()}</p>
      </Show>

      {/* Opened-repo tabs, grouped */}
      <Show when={opened().length > 0}>
        <div style={{ "margin-top": "0.5rem", display: "flex", "flex-direction": "column", gap: "0.3rem" }}>
          <For each={groups()}>
            {(g) => (
              <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "flex-wrap": "wrap" }}>
                <span style={{ color: "var(--fg-muted)", "font-size": "0.72rem", "min-width": "5rem" }}>
                  {g ?? "Ungrouped"}
                </span>
                <For each={opened().filter((r) => (r.group ?? null) === g)}>
                  {(repo) => (
                    <button style={tabStyle(repo)} onClick={() => activate(repo)} title={repo.path}>
                      <Show when={repo.dirty}>
                        <span style={{ "margin-right": "0.25rem" }}>★</span>
                      </Show>
                      {baseName(repo.path)}
                    </button>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </Show>

      {/* Recent repositories */}
      <Show when={recent().length > 0}>
        <div style={{ "margin-top": "0.4rem", display: "flex", gap: "0.4rem", "align-items": "center", "flex-wrap": "wrap" }}>
          <span style={{ color: "var(--fg-muted)", "font-size": "0.72rem" }}>Recent:</span>
          <For each={recent()}>
            {(r) => (
              <button
                onClick={() => openPath(r.path, r.group)}
                title={r.path}
                style={{
                  padding: "0.15rem 0.5rem",
                  "font-size": "0.78rem",
                  border: "1px solid var(--border)",
                  "border-radius": "4px",
                  background: "var(--surface-2)",
                  cursor: "pointer",
                }}
              >
                {baseName(r.path)}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default RepoBar;
