import { createEffect, createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, ApplyOutcome, OpenRepo, RefInfo } from "./commands";
import GraphView from "./GraphView";
import DiffView from "./DiffView";
import BlameView from "./BlameView";
import FileHistory from "./FileHistory";
import ChangesView from "./ChangesView";
import RefsView from "./RefsView";
import SyncBar from "./SyncBar";
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
  const [opNotice, setOpNotice] = createSignal<string | null>(null);
  const [opConflicts, setOpConflicts] = createSignal<string[]>([]);
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

  const describeApply = (action: string, outcome: ApplyOutcome) => {
    if (outcome.kind === "Applied") return `${action} applied as ${outcome.value.slice(0, 8)}.`;
    return `${action} stopped with ${outcome.value.length} conflict${outcome.value.length === 1 ? "" : "s"}.`;
  };

  const runCommitAction = (cmd: "cherry_pick" | "revert", label: string) => {
    const repo = active();
    const oid = selectedCommit();
    if (!repo || !oid) return;
    if (cmd === "revert" && !confirm(`Revert commit ${oid.slice(0, 8)}?`)) return;
    setErr(null);
    setOpNotice(null);
    setOpConflicts([]);
    invoke<ApplyOutcome>(cmd, { repo: repo.id, oid })
      .then((outcome) => {
        if (outcome.kind === "Conflicts") setOpConflicts(outcome.value);
        setOpNotice(describeApply(label, outcome));
        refresh();
      })
      .catch((e) => setErr(String(e)));
  };

  const abortSequencer = () => {
    const repo = active();
    if (!repo) return;
    setErr(null);
    invoke("sequencer_abort", { repo: repo.id })
      .then(() => {
        setOpConflicts([]);
        setOpNotice("Operation aborted.");
        refresh();
      })
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
      <Show when={opNotice()}>
        <p style={{ color: "#1a7f37", margin: "0.25rem 1rem", "font-size": "0.85rem" }}>{opNotice()}</p>
      </Show>
      <Show when={opConflicts().length > 0}>
        <div style={{ margin: "0.25rem 1rem", padding: "0.35rem", border: "1px solid #f0c36d", background: "#fff8e5", "font-size": "0.8rem" }}>
          <span style={{ "font-weight": 700 }}>Conflicts: </span>
          <span>{opConflicts().join(", ")}</span>
          <button style={{ margin: "0 0 0 0.5rem" }} onClick={abortSequencer}>
            Abort
          </button>
        </div>
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

      {/* Remote sync: fetch / pull / push + ahead/behind */}
      <Show when={active()}>
        <SyncBar repoId={repoId()!} refreshNonce={refreshNonce()} onChanged={refresh} />
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
                    display: "flex",
                    "flex-direction": "column",
                  }}
                >
                  <div style={{ display: "flex", gap: "0.4rem", padding: "0.35rem", "border-bottom": "1px solid #eee", "font-size": "0.8rem" }}>
                    <button onClick={() => runCommitAction("cherry_pick", "Cherry-pick")}>
                      Cherry-pick
                    </button>
                    <button onClick={() => runCommitAction("revert", "Revert")}>
                      Revert
                    </button>
                  </div>
                  <div style={{ flex: "1", "min-height": "0" }}>
                    <DiffView repoId={repoId()!} commit={selectedCommit()!} />
                  </div>
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
