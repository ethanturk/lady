import { createEffect, createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, ApplyOutcome, ConflictState, OpenRepo, RebaseOutcome, RefInfo } from "./commands";
import GraphView from "./GraphView";
import DiffView from "./DiffView";
import BlameView from "./BlameView";
import FileHistory from "./FileHistory";
import ChangesView from "./ChangesView";
import RefsView from "./RefsView";
import SyncBar from "./SyncBar";
import RepoBar from "./RepoBar";
import ConflictResolver from "./ConflictResolver";
import InteractiveRebase from "./InteractiveRebase";
import WorktreesView from "./WorktreesView";
import ReflogView from "./ReflogView";
import BisectView from "./BisectView";
import CustomCommandsView from "./CustomCommandsView";
import SettingsView from "./SettingsView";
import NotificationsView from "./NotificationsView";
import LfsView from "./LfsView";
import GitFlowView from "./GitFlowView";
import SubmodulesView from "./SubmodulesView";
import AiView from "./AiView";
import LicenseGate from "./LicenseGate";
import ThemeToggle from "./ThemeToggle";
import SignatureBadge from "./SignatureBadge";
import type { LicenseStatus, SignatureStatus } from "./commands";
import CommandPalette from "./CommandPalette";
import type { PaletteEntry } from "./CommandPalette";

type Tab =
  | "changes"
  | "commits"
  | "refs"
  | "blame"
  | "history"
  | "conflicts"
  | "worktrees"
  | "reflog"
  | "bisect"
  | "commands"
  | "settings"
  | "notifications"
  | "lfs"
  | "flow"
  | "submodules"
  | "ai";

const App: Component = () => {
  const [info, setInfo] = createSignal<AppInfo | null>(null);
  const [license, setLicense] = createSignal<LicenseStatus | null>(null);
  const [unread, setUnread] = createSignal(0);
  const [active, setActive] = createSignal<OpenRepo | null>(null);
  const [refs, setRefs] = createSignal<RefInfo[]>([]);
  const [tab, setTab] = createSignal<Tab>("changes");
  const [selectedCommit, setSelectedCommit] = createSignal<string | null>(null);
  const [selectedSig, setSelectedSig] = createSignal<SignatureStatus | undefined>(undefined);
  const [files, setFiles] = createSignal<string[]>([]);
  const [navFile, setNavFile] = createSignal<string | undefined>(undefined);
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);
  const [opNotice, setOpNotice] = createSignal<string | null>(null);
  const [opConflicts, setOpConflicts] = createSignal<string[]>([]);
  const [conflictState, setConflictState] = createSignal<ConflictState>("None");
  // When set, the interactive-rebase editor is open for this start commit oid.
  const [rebaseFrom, setRebaseFrom] = createSignal<string | null>(null);
  // Opener handed up from RepoBar so a worktree can be opened as a repo tab.
  let openRepoPath: ((path: string) => void) | null = null;
  // Bumped after any mutation so status/refs/graph views reload (PLAN §3.2).
  const [refreshNonce, setRefreshNonce] = createSignal(0);
  // Poll the mid-operation state; auto-surface the resolver when conflicts
  // appear (PH3-002: merge / cherry-pick / revert / rebase report conflicts).
  const updateConflictState = (repo: OpenRepo) => {
    invoke<ConflictState>("conflict_state", { repo: repo.id })
      .then((s) => {
        const wasIdle = conflictState() === "None";
        setConflictState(s);
        if (s !== "None") {
          invoke<string[]>("list_conflicts", { repo: repo.id })
            .then((c) => {
              if (c.length > 0 && wasIdle) setTab("conflicts");
            })
            .catch(() => {});
        } else if (tab() === "conflicts") {
          setTab("changes");
        }
      })
      .catch(() => {});
  };
  const refresh = () => {
    setRefreshNonce((n) => n + 1);
    const repo = active();
    if (!repo) return;
    invoke<RefInfo[]>("list_refs", { repo: repo.id })
      .then(setRefs)
      .catch((e) => setErr(String(e)));
    updateConflictState(repo);
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

  // Result of running an interactive rebase: finish cleanly, or hand a conflict
  // / edit-stop off to the 3-pane resolver (PH3-002).
  const onRebaseComplete = (outcome: RebaseOutcome) => {
    setRebaseFrom(null);
    setErr(null);
    if (outcome.kind === "Rebased") {
      setOpNotice("Rebase complete.");
    } else if (outcome.kind === "Stopped") {
      setOpNotice("Rebase stopped to edit a commit — amend, then continue.");
      setTab("conflicts");
    } else {
      setOpConflicts(outcome.value);
      setOpNotice(`Rebase stopped with ${outcome.value.length} conflict(s).`);
      setTab("conflicts");
    }
    refresh();
  };

  // Fetch the selected commit's signature status for the details badge.
  createEffect(() => {
    const repo = active();
    const oid = selectedCommit();
    setSelectedSig(undefined);
    if (!repo || !oid) return;
    invoke<SignatureStatus[]>("signature_statuses", { repo: repo.id, oids: [oid] })
      .then((s) => setSelectedSig(s[0]))
      .catch(() => {});
  });

  onMount(async () => {
    const data = await invoke<AppInfo>("app_info");
    setInfo(data);
    invoke<LicenseStatus>("license_status").then(setLicense).catch(() => {});
    // Background poll for the notifications badge (best-effort; needs a GitHub
    // token, otherwise silently stays at 0).
    const pollUnread = () =>
      invoke<{ unread: boolean }[]>("github_notifications")
        .then((n) => setUnread(n.filter((x) => x.unread).length))
        .catch(() => {});
    pollUnread();
    setInterval(pollUnread, 60_000);
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
    setConflictState("None");
    updateConflictState(repo);
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
    background: tab() === t ? "var(--accent)" : "var(--border)",
    color: tab() === t ? "var(--on-accent)" : "var(--fg)",
    "border-radius": "4px 4px 0 0",
    "font-size": "0.875rem",
  });

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        "flex-direction": "column",
        "font-family": "sans-serif",
        overflow: "hidden",
      }}
    >
      {/* App title + trial banner */}
      <div style={{ padding: "0.5rem 1rem 0", "flex-shrink": 0, display: "flex", "align-items": "center", gap: "0.6rem" }}>
        <Show when={info()}>
          <span style={{ "font-weight": 600 }}>
            {info()?.name} {info()?.version}
          </span>
        </Show>
        <Show when={license()?.kind === "Trial"}>
          <span style={{ background: "#fff8c5", color: "#9a6700", "border-radius": "3px", padding: "0.05rem 0.5rem", "font-size": "0.75rem" }}>
            Trial — {(license() as { days_left: number }).days_left} day
            {(license() as { days_left: number }).days_left === 1 ? "" : "s"} left
          </span>
        </Show>
        <Show when={license()?.kind === "Licensed"}>
          <span style={{ color: "#1a7f37", "font-size": "0.72rem" }}>● Licensed</span>
        </Show>
        <span style={{ flex: "1" }} />
        <ThemeToggle />
      </div>

      {/* Licensing gate: blocks the UI when the trial has expired (ADR-0007). */}
      <Show when={license()?.kind === "Expired"}>
        <LicenseGate onActivated={setLicense} />
      </Show>

      {/* Repository manager */}
      <RepoBar active={repoId()} onActiveChange={setActive} apiRef={(open) => (openRepoPath = open)} />

      <Show when={err()}>
        <p style={{ color: "var(--error)", margin: "0.25rem 1rem", "font-size": "0.85rem" }}>{err()}</p>
      </Show>
      <Show when={opNotice()}>
        <p style={{ color: "#1a7f37", margin: "0.25rem 1rem", "font-size": "0.85rem" }}>{opNotice()}</p>
      </Show>
      <Show when={opConflicts().length > 0}>
        <div style={{ margin: "0.25rem 1rem", padding: "0.35rem", border: "1px solid #f0c36d", background: "#fff8e5", "font-size": "0.8rem" }}>
          <span style={{ "font-weight": 700 }}>Conflicts: </span>
          <span>{opConflicts().join(", ")}</span>
          <button style={{ margin: "0 0 0 0.5rem" }} onClick={() => setTab("conflicts")}>
            Resolve
          </button>
          <button style={{ margin: "0 0 0 0.5rem" }} onClick={abortSequencer}>
            Abort
          </button>
        </div>
      </Show>

      {/* View tabs for the active repo */}
      <Show when={active()}>
        <div style={{ display: "flex", "flex-wrap": "wrap", gap: "0.25rem", padding: "0.5rem 1rem 0", "flex-shrink": 0 }}>
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
          <button style={tabStyle("worktrees")} onClick={() => setTab("worktrees")}>
            Worktrees
          </button>
          <button style={tabStyle("reflog")} onClick={() => setTab("reflog")}>
            Reflog
          </button>
          <button style={tabStyle("bisect")} onClick={() => setTab("bisect")}>
            Bisect
          </button>
          <button style={tabStyle("commands")} onClick={() => setTab("commands")}>
            Commands
          </button>
          <button style={tabStyle("lfs")} onClick={() => setTab("lfs")}>
            LFS
          </button>
          <button style={tabStyle("flow")} onClick={() => setTab("flow")}>
            git-flow
          </button>
          <button style={tabStyle("submodules")} onClick={() => setTab("submodules")}>
            Submodules
          </button>
          <button style={tabStyle("ai")} onClick={() => setTab("ai")}>
            ✨ AI
          </button>
          <button style={tabStyle("settings")} onClick={() => setTab("settings")}>
            Settings
          </button>
          <button style={tabStyle("notifications")} onClick={() => setTab("notifications")}>
            Notifications
            <Show when={unread() > 0}>
              <span style={{ "margin-left": "0.3rem", background: "#cf222e", color: "var(--on-accent)", "border-radius": "8px", padding: "0 0.35rem", "font-size": "0.7rem" }}>
                {unread()}
              </span>
            </Show>
          </button>
          <Show when={conflictState() !== "None"}>
            <button
              style={{ ...tabStyle("conflicts"), background: tab() === "conflicts" ? "#d1242f" : "#ffe0e0", color: tab() === "conflicts" ? "var(--on-accent)" : "#d1242f" }}
              onClick={() => setTab("conflicts")}
            >
              Conflicts ⚠
            </button>
          </Show>
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
                    "border-left": "1px solid var(--border)",
                    overflow: "hidden",
                    display: "flex",
                    "flex-direction": "column",
                  }}
                >
                  <div style={{ display: "flex", gap: "0.4rem", padding: "0.35rem", "border-bottom": "1px solid var(--border)", "font-size": "0.8rem" }}>
                    <button onClick={() => runCommitAction("cherry_pick", "Cherry-pick")}>
                      Cherry-pick
                    </button>
                    <button onClick={() => runCommitAction("revert", "Revert")}>
                      Revert
                    </button>
                    <button onClick={() => setRebaseFrom(selectedCommit())}>
                      Rebase i from here
                    </button>
                    <span style={{ flex: "1" }} />
                    <SignatureBadge status={selectedSig()} />
                  </div>
                  <div style={{ flex: "1", "min-height": "0" }}>
                    <DiffView repoId={repoId()!} commit={selectedCommit()!} />
                  </div>
                </div>
              </Show>
            </div>
          </Show>
          <Show when={tab() === "refs"}>
            <RefsView
              repoId={repoId()!}
              refs={refs()}
              onChanged={refresh}
              onInteractiveRebase={(oid) => setRebaseFrom(oid)}
            />
          </Show>
          <Show when={tab() === "blame"}>
            <BlameView repoId={repoId()!} initialPath={navFile()} />
          </Show>
          <Show when={tab() === "history"}>
            <FileHistory repoId={repoId()!} />
          </Show>
          <Show when={tab() === "worktrees"}>
            <WorktreesView
              repoId={repoId()!}
              refreshNonce={refreshNonce()}
              onChanged={refresh}
              onOpen={(path) => openRepoPath?.(path)}
            />
          </Show>
          <Show when={tab() === "reflog"}>
            <ReflogView repoId={repoId()!} refreshNonce={refreshNonce()} onChanged={refresh} />
          </Show>
          <Show when={tab() === "bisect"}>
            <BisectView repoId={repoId()!} refreshNonce={refreshNonce()} onChanged={refresh} />
          </Show>
          <Show when={tab() === "commands"}>
            <CustomCommandsView repoId={repoId()!} refs={refs()} files={files()} />
          </Show>
          <Show when={tab() === "lfs"}>
            <LfsView repoId={repoId()!} refreshNonce={refreshNonce()} onChanged={refresh} />
          </Show>
          <Show when={tab() === "flow"}>
            <GitFlowView repoId={repoId()!} refreshNonce={refreshNonce()} onChanged={refresh} />
          </Show>
          <Show when={tab() === "submodules"}>
            <SubmodulesView
              repoId={repoId()!}
              repoPath={active()!.path}
              refreshNonce={refreshNonce()}
              onChanged={refresh}
              onOpen={(path) => openRepoPath?.(path)}
            />
          </Show>
          <Show when={tab() === "ai"}>
            <AiView repoId={repoId()!} onChanged={refresh} />
          </Show>
          <Show when={tab() === "settings"}>
            <SettingsView repoId={repoId()!} />
          </Show>
          <Show when={tab() === "notifications"}>
            <NotificationsView refreshNonce={refreshNonce()} onUnread={setUnread} />
          </Show>
          <Show when={tab() === "conflicts"}>
            <ConflictResolver
              repoId={repoId()!}
              refreshNonce={refreshNonce()}
              conflictState={conflictState()}
              onChanged={refresh}
              onDone={() => {
                setOpConflicts([]);
                setOpNotice("All conflicts resolved. Commit to finish the operation.");
                refresh();
              }}
            />
          </Show>
        </div>
      </Show>

      {/* Interactive-rebase editor (PH3-004) */}
      <Show when={rebaseFrom() && active()}>
        <InteractiveRebase
          repoId={repoId()!}
          fromOid={rebaseFrom()!}
          onClose={() => setRebaseFrom(null)}
          onComplete={onRebaseComplete}
        />
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
