import { createEffect, createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, OpenRepo, RefInfo } from "./commands";
import GraphView from "./GraphView";
import DiffView from "./DiffView";
import BlameView from "./BlameView";
import FileHistory from "./FileHistory";
import ChangesView from "./ChangesView";
import RefsView from "./RefsView";
import RepoBar from "./RepoBar";
import CommandPalette from "./CommandPalette";
import type { PaletteEntry } from "./CommandPalette";

type Tab = "changes" | "commits" | "refs" | "blame" | "history";

const App: Component = () => {
  const [info, setInfo] = createSignal<AppInfo | null>(null);
  const [active, setActive] = createSignal<OpenRepo | null>(null);
  const [refs, setRefs] = createSignal<RefInfo[]>([]);
  const [tab, setTab] = createSignal<Tab>("changes");
  const [selectedCommit, setSelectedCommit] = createSignal<string | null>(null);
  const [files, setFiles] = createSignal<string[]>([]);
  const [navFile, setNavFile] = createSignal<string | undefined>(undefined);
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  // Bumped after any mutation so status/refs/graph views reload (PLAN §3.2).
  const [refreshNonce, setRefreshNonce] = createSignal(0);
  const refresh = () => {
    setRefreshNonce((n) => n + 1);
    const repo = active();
    if (!repo) return;
    invoke<RefInfo[]>("list_refs", { repo: repo.id })
      .then(setRefs)
      .catch((e) => setErr(String(e)));
  };

  onMount(async () => {
    const data = await invoke<AppInfo>("app_info");
    setInfo(data);
  });

  const repoId = () => active()?.id ?? null;

  // Reload refs + file list whenever the active repo changes.
  createEffect(() => {
    const repo = active();
    setSelectedCommit(null);
    setRefs([]);
    setFiles([]);
    if (!repo) return;
    invoke<RefInfo[]>("list_refs", { repo: repo.id })
      .then(setRefs)
      .catch((e) => setErr(String(e)));
    invoke<string[]>("list_files", { repo: repo.id })
      .then(setFiles)
      .catch(() => setFiles([]));
  });

  // Palette entries: tab actions + branches (→ Refs) + files (→ Blame).
  const paletteEntries = (): PaletteEntry[] => {
    const actions: PaletteEntry[] = [
      { kind: "action", label: "Go to Changes", run: () => setTab("changes") },
      { kind: "action", label: "Go to Commits", run: () => setTab("commits") },
      { kind: "action", label: "Go to Refs", run: () => setTab("refs") },
      { kind: "action", label: "Go to Blame", run: () => setTab("blame") },
      { kind: "action", label: "Go to File History", run: () => setTab("history") },
    ];
    const branches: PaletteEntry[] = refs()
      .filter((r) => r.kind === "Branch" || r.kind === "Remote")
      .map((r) => ({ kind: "branch", label: r.name, run: () => setTab("refs") }));
    const fileEntries: PaletteEntry[] = files().map((f) => ({
      kind: "file",
      label: f,
      run: () => {
        setNavFile(f);
        setTab("blame");
      },
    }));
    return [...actions, ...branches, ...fileEntries];
  };

  const tabStyle = (t: Tab) => ({
    padding: "0.3rem 0.9rem",
    cursor: "pointer",
    border: "none",
    background: tab() === t ? "#0070f3" : "#eee",
    color: tab() === t ? "#fff" : "#333",
    "border-radius": "4px 4px 0 0",
    "font-size": "0.875rem",
  });

  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        "flex-direction": "column",
        "font-family": "sans-serif",
      }}
    >
      {/* App title */}
      <div style={{ padding: "0.5rem 1rem 0", "flex-shrink": 0 }}>
        <Show when={info()}>
          <span style={{ "font-weight": 600 }}>
            {info()?.name} {info()?.version}
          </span>
        </Show>
      </div>

      {/* Repository manager */}
      <RepoBar active={repoId()} onActiveChange={setActive} />

      <Show when={err()}>
        <p style={{ color: "crimson", margin: "0.25rem 1rem", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      {/* View tabs for the active repo */}
      <Show when={active()}>
        <div style={{ display: "flex", gap: "0.25rem", padding: "0.5rem 1rem 0", "flex-shrink": 0 }}>
          <button style={tabStyle("changes")} onClick={() => setTab("changes")}>
            Changes
          </button>
          <button style={tabStyle("commits")} onClick={() => setTab("commits")}>
            Commits
          </button>
          <button style={tabStyle("refs")} onClick={() => setTab("refs")}>
            Refs
          </button>
          <button style={tabStyle("blame")} onClick={() => setTab("blame")}>
            Blame
          </button>
          <button style={tabStyle("history")} onClick={() => setTab("history")}>
            History
          </button>
        </div>
      </Show>

      {/* Content */}
      <Show when={active()}>
        <div style={{ flex: "1", overflow: "hidden" }}>
          <Show when={tab() === "changes"}>
            <ChangesView
              repoId={repoId()!}
              refreshNonce={refreshNonce()}
              onChanged={refresh}
            />
          </Show>
          <Show when={tab() === "commits"}>
            <div style={{ display: "flex", height: "100%", overflow: "hidden" }}>
              <div style={{ flex: "1", "min-width": "0", overflow: "hidden" }}>
                <GraphView
                  repoId={repoId()!}
                  selected={selectedCommit() ?? undefined}
                  onSelectCommit={setSelectedCommit}
                />
              </div>
              <Show when={selectedCommit()}>
                <div
                  style={{
                    flex: "1",
                    "min-width": "0",
                    "border-left": "1px solid #ddd",
                    overflow: "hidden",
                  }}
                >
                  <DiffView repoId={repoId()!} commit={selectedCommit()!} />
                </div>
              </Show>
            </div>
          </Show>
          <Show when={tab() === "refs"}>
            <RefsView repoId={repoId()!} refs={refs()} onChanged={refresh} />
          </Show>
          <Show when={tab() === "blame"}>
            <BlameView repoId={repoId()!} initialPath={navFile()} />
          </Show>
          <Show when={tab() === "history"}>
            <FileHistory repoId={repoId()!} />
          </Show>
        </div>
      </Show>

      <CommandPalette
        open={paletteOpen()}
        entries={paletteEntries()}
        onOpen={() => setPaletteOpen(true)}
        onClose={() => setPaletteOpen(false)}
      />
    </div>
  );
};

export default App;
