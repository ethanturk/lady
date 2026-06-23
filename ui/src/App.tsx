import { createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AccountSuggestion, ApplyOutcome, ConflictState, OpenRepo, RebaseOutcome, RecentRepo, RefInfo, RepositoryFamily, WorkingTree } from "./commands";
import { assignRepoAccount, dismissRepoAccountSuggestion, suggestRepoAccount } from "./accounts";
import AllCommitsView from "./AllCommitsView";
import BlameView from "./BlameView";
import FileHistory from "./FileHistory";
import ChangesView from "./ChangesView";
import RefsView from "./RefsView";
import RepoBar from "./RepoBar";
import Toolbar from "./Toolbar";
import type { OverflowItem } from "./Toolbar";
import Sidebar from "./Sidebar";
import type { PrimaryView } from "./Sidebar";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import BranchMenu from "./BranchMenu";
import type { BranchMenuState, PromptSpec } from "./BranchMenu";
import CommitMenu from "./CommitMenu";
import type { CommitMenuState } from "./CommitMenu";
import TagMenu from "./TagMenu";
import type { TagMenuState } from "./TagMenu";
import PushDialog from "./PushDialog";
import type { PushDialogState } from "./PushDialog";
import { addWorktreeFor, checkoutBranch, createBranchAt, createTagAt, deleteBranch } from "./branchActions";
import { autoUpdateCheck, hideResizers, isNarrow, setSettingsWidth, setSidebarWidth, settingsWidth, sidebarWidth } from "./prefs";
import ConflictResolver from "./ConflictResolver";
import InteractiveRebase from "./InteractiveRebase";
import RecomposeView from "./RecomposeView";
import ExplainPanel from "./ExplainPanel";
import WorktreesView from "./WorktreesView";
import ReflogView from "./ReflogView";
import BisectView from "./BisectView";
import CustomCommandsView from "./CustomCommandsView";
import SettingsView from "./SettingsView";
import NotificationsView from "./NotificationsView";
import LfsView from "./LfsView";
import GitFlowView from "./GitFlowView";
import SubmodulesView from "./SubmodulesView";
import StashView from "./StashView";
import AiView from "./AiView";
import LicenseGate from "./LicenseGate";
import type { LicenseStatus, SignatureStatus } from "./commands";
import CommandPalette from "./CommandPalette";
import type { PaletteEntry } from "./CommandPalette";

/** Advanced views, reached from the toolbar "More" menu (overlay the main area). */
type Overlay =
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
  | "stashes"
  | "ai";

/** Last path segment, for the repo title in the toolbar/sidebar. */
const baseName = (path: string) =>
  path.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || path;

const App: Component = () => {
  const [license, setLicense] = createSignal<LicenseStatus | null>(null);
  const [unread, setUnread] = createSignal(0);
  const [active, setActive] = createSignal<OpenRepo | null>(null);
  const [refs, setRefs] = createSignal<RefInfo[]>([]);
  const [repositoryFamily, setRepositoryFamily] = createSignal<RepositoryFamily | null>(null);
  // Primary nav (sidebar) vs. an overlaid advanced view (toolbar "More").
  const [view, setView] = createSignal<PrimaryView>("changes");
  const [overlay, setOverlay] = createSignal<Overlay | null>(null);
  const [changeCount, setChangeCount] = createSignal(0);
  const [branchMenu, setBranchMenu] = createSignal<BranchMenuState | null>(null);
  const [commitMenu, setCommitMenu] = createSignal<CommitMenuState | null>(null);
  const [tagMenu, setTagMenu] = createSignal<TagMenuState | null>(null);
  // "New branch from <startPoint>" modal (replaces the unsupported window.prompt).
  const [newBranchFrom, setNewBranchFrom] = createSignal<string | null>(null);
  const [newBranchName, setNewBranchName] = createSignal("");
  // Generalized name prompt (Rename, New Tag, …) — also avoids window.prompt.
  const [promptSpec, setPromptSpec] = createSignal<PromptSpec | null>(null);
  const [promptValue, setPromptValue] = createSignal("");
  // Commit selection (multi-select): all selected oids + the primary (last
  // clicked) that drives the detail pane and single-commit actions.
  const [selectedCommits, setSelectedCommits] = createSignal<string[]>([]);
  const [primaryCommit, setPrimaryCommit] = createSignal<string | null>(null);
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
  // When set, the AI recompose overlay is open for this start commit oid.
  const [recomposeFrom, setRecomposeFrom] = createSignal<string | null>(null);
  // When set, the AI "Explain changes" overlay is open for this target.
  const [explainSpec, setExplainSpec] = createSignal<{ target: Record<string, unknown>; title: string; subtitle?: string } | null>(null);
  // Push confirmation dialog (shown for every push operation).
  const [pushDialog, setPushDialog] = createSignal<PushDialogState | null>(null);
  // Recent repositories (from RepoBar) shown in the Launch palette.
  const [recents, setRecents] = createSignal<RecentRepo[]>([]);
  // Openers handed up from RepoBar: open a known path (worktrees) + native picker.
  let openRepoPath: ((path: string) => void) | null = null;
  let repoPicker: (() => void) | null = null;
  // Bumped after any mutation so status/refs/graph views reload (PLAN §3.2).
  const [refreshNonce, setRefreshNonce] = createSignal(0);
  // Off-canvas sidebar drawer (phone/narrow only). Auto-closes when the layout
  // grows back to side-by-side, and on navigation (see goPrimary).
  const [drawerOpen, setDrawerOpen] = createSignal(false);
  createEffect(() => {
    if (!isNarrow()) setDrawerOpen(false);
  });

  const repoId = () => active()?.id ?? null;
  const repoName = () => (active() ? baseName(active()!.path) : null);

  // Confirm-once GitHub account suggestion: when a repo with no auth override is
  // opened and its remote owner matches a known account, offer to pin it.
  const [acctSuggestion, setAcctSuggestion] = createSignal<AccountSuggestion | null>(null);

  // Launch-time update prompt (PH6-008). The check runs on mount when enabled;
  // installing stays an explicit click (banner button), never silent.
  type UpdateInfo = { available: boolean; version: string | null; notes: string | null; current: string };
  const [updateAvail, setUpdateAvail] = createSignal<UpdateInfo | null>(null);
  const [updateBusy, setUpdateBusy] = createSignal(false);
  const installLaunchUpdate = () => {
    if (!confirm("Download and install the update now? Lady will restart.")) return;
    setUpdateBusy(true);
    // On success the app restarts mid-call; we mainly surface failures.
    invoke("install_update").catch((e) => {
      setErr(String(e));
      setUpdateBusy(false);
    });
  };
  createEffect(() => {
    const repo = repoId();
    setAcctSuggestion(null);
    if (!repo) return;
    suggestRepoAccount(repo).then(setAcctSuggestion).catch(() => {});
  });
  const acceptSuggestion = () => {
    const repo = repoId();
    const s = acctSuggestion();
    if (!repo || !s) return;
    assignRepoAccount(repo, s.account.id)
      .then(() => setAcctSuggestion(null))
      .catch((e) => setErr(String(e)));
  };
  const dismissSuggestion = () => {
    const repo = repoId();
    if (!repo) return;
    dismissRepoAccountSuggestion(repo).catch(() => {});
    setAcctSuggestion(null);
  };
  // The Head ref's name is the checked-out branch ("HEAD" when detached).
  const currentBranchName = () => {
    const h = refs().find((r) => r.kind === "Head")?.name;
    return h && h !== "HEAD" ? h : null;
  };
  // Best-effort default branch for "branch-into-tag" operations: main, master,
  // or the first local branch. Falls back to the current branch if no branches
  // are known, so the menu items remain operational.
  const defaultBranch = () => {
    const names = refs()
      .filter((r) => r.kind === "Branch")
      .map((r) => r.name);
    if (names.includes("main")) return "main";
    if (names.includes("master")) return "master";
    return names[0] ?? currentBranchName() ?? "main";
  };

  const loadChangeCount = (repo: string) => {
    invoke<WorkingTree>("status", { repo })
      .then((wt) => setChangeCount(wt.staged.length + wt.unstaged.length + wt.untracked.length))
      .catch(() => setChangeCount(0));
  };

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
              if (c.length > 0 && wasIdle) setOverlay("conflicts");
            })
            .catch(() => {});
        } else if (overlay() === "conflicts") {
          setOverlay(null);
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
    invoke<RepositoryFamily>("repository_family", { repo: repo.id })
      .then(setRepositoryFamily)
      .catch(() => setRepositoryFamily(null));
    loadChangeCount(repo.id);
    updateConflictState(repo);
  };

  // Poll local repo state so edits made outside Lady (editor, terminal) show up
  // without clicking Fetch. Pauses while the window is hidden.
  const REPO_POLL_MS = 2_000;
  const REMOTE_FETCH_MS = 5_000;
  createEffect(() => {
    if (!active()) return;
    const tick = () => {
      if (document.visibilityState !== "hidden") refresh();
    };
    const id = window.setInterval(tick, REPO_POLL_MS);
    const onVis = () => {
      if (document.visibilityState === "visible") refresh();
    };
    document.addEventListener("visibilitychange", onVis);
    onCleanup(() => {
      clearInterval(id);
      document.removeEventListener("visibilitychange", onVis);
    });
  });

  // Keep remote-tracking branches fresh for incoming/outgoing branch badges.
  // This intentionally uses a quiet backend command so the toolbar progress line
  // is reserved for user-initiated Fetch/Pull/Push operations.
  createEffect(() => {
    const repo = active();
    if (!repo) return;
    let fetching = false;
    const tick = async () => {
      if (fetching || document.visibilityState === "hidden") return;
      fetching = true;
      try {
        await invoke("fetch_background", { repo: repo.id });
        if (active()?.id === repo.id) refresh();
      } catch {
        // Background fetch is best-effort; explicit Fetch still reports errors.
      } finally {
        fetching = false;
      }
    };
    const id = window.setInterval(tick, REMOTE_FETCH_MS);
    const onVis = () => {
      if (document.visibilityState === "visible") void tick();
    };
    void tick();
    document.addEventListener("visibilitychange", onVis);
    onCleanup(() => {
      clearInterval(id);
      document.removeEventListener("visibilitychange", onVis);
    });
  });

  const describeApply = (action: string, outcome: ApplyOutcome) => {
    if (outcome.kind === "Applied") return `${action} applied as ${outcome.value.slice(0, 8)}.`;
    return `${action} stopped with ${outcome.value.length} conflict${outcome.value.length === 1 ? "" : "s"}.`;
  };

  const runCommitAction = (cmd: "cherry_pick" | "revert", label: string) => {
    const repo = active();
    const oid = primaryCommit();
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
      setOverlay("conflicts");
    } else {
      setOpConflicts(outcome.value);
      setOpNotice(`Rebase stopped with ${outcome.value.length} conflict(s).`);
      setOverlay("conflicts");
    }
    refresh();
  };

  // Fetch the selected commit's signature status for the details badge.
  createEffect(() => {
    const repo = active();
    const oid = primaryCommit();
    setSelectedSig(undefined);
    if (!repo || !oid) return;
    invoke<SignatureStatus[]>("signature_statuses", { repo: repo.id, oids: [oid] })
      .then((s) => setSelectedSig(s[0]))
      .catch(() => {});
  });

  // Status notices ("Checked out …", "Merged …") auto-dismiss as a transient
  // toast: hold briefly, fade out, then clear so they never accrue on screen.
  const [toastLeaving, setToastLeaving] = createSignal(false);
  let toastTimers: number[] = [];
  createEffect(() => {
    const msg = opNotice();
    toastTimers.forEach(clearTimeout);
    toastTimers = [];
    setToastLeaving(false);
    if (!msg) return;
    toastTimers.push(window.setTimeout(() => setToastLeaving(true), 3000));
    toastTimers.push(window.setTimeout(() => setOpNotice(null), 3400));
  });
  onCleanup(() => toastTimers.forEach(clearTimeout));

  onMount(() => {
    invoke<LicenseStatus>("license_status").then(setLicense).catch(() => {});
    // Background poll for the notifications badge (best-effort; needs a GitHub
    // token, otherwise silently stays at 0).
    const pollUnread = () =>
      invoke<{ unread: boolean }[]>("github_notifications")
        .then((n) => setUnread(n.filter((x) => x.unread).length))
        .catch(() => {});
    pollUnread();
    setInterval(pollUnread, 60_000);
    // Launch-time update check (opt-out via Settings). Read-only — only flips
    // the banner; the user installs explicitly. Failures stay silent.
    if (autoUpdateCheck()) {
      invoke<UpdateInfo>("check_for_updates")
        .then((info) => {
          if (info.available) setUpdateAvail(info);
        })
        .catch(() => {});
    }
  });

  // Reload refs + file list whenever the active repo changes; reset navigation.
  createEffect(() => {
    const repo = active();
    setSelectedCommits([]);
    setPrimaryCommit(null);
    setRefs([]);
    setRepositoryFamily(null);
    setFiles([]);
    setView("changes");
    setOverlay(null);
    setChangeCount(0);
    if (!repo) return;
    invoke<RefInfo[]>("list_refs", { repo: repo.id })
      .then(setRefs)
      .catch((e) => setErr(String(e)));
    invoke<RepositoryFamily>("repository_family", { repo: repo.id })
      .then(setRepositoryFamily)
      .catch(() => setRepositoryFamily(null));
    invoke<string[]>("list_files", { repo: repo.id })
      .then(setFiles)
      .catch(() => setFiles([]));
    loadChangeCount(repo.id);
    setConflictState("None");
    updateConflictState(repo);
  });

  // Palette entries: recent repos first (the Launch menu), then nav actions +
  // branches (→ Refs) + files (→ Blame).
  const paletteEntries = (): PaletteEntry[] => {
    const repos: PaletteEntry[] = recents().map((r) => ({
      kind: "repo",
      label: `${r.family_name ?? baseName(r.path)}   ${r.path}`,
      run: () => openRepoPath?.(r.path),
    }));
    const actions: PaletteEntry[] = [
      { kind: "action", label: "Go to Local Changes", run: () => goPrimary("changes") },
      { kind: "action", label: "Go to All Commits", run: () => goPrimary("commits") },
      { kind: "action", label: "Refs / Pull Requests", run: () => setOverlay("refs") },
      { kind: "action", label: "Blame", run: () => setOverlay("blame") },
      { kind: "action", label: "File History", run: () => setOverlay("history") },
      { kind: "action", label: "Create Worktree...", run: () => setOverlay("worktrees") },
      { kind: "action", label: "Settings", run: () => setOverlay("settings") },
    ];
    const worktrees: PaletteEntry[] = (repositoryFamily()?.worktrees ?? [])
      .filter((wt) => !wt.selected && !wt.missing && !wt.prunable)
      .map((wt) => ({
        kind: "action",
        label: `Switch Worktree: ${wt.display_name}`,
        run: () => openRepoPath?.(wt.path),
      }));
    const branches: PaletteEntry[] = refs()
      .filter((r) => r.kind === "Branch" || r.kind === "Remote")
      .map((r) => ({ kind: "branch", label: r.name, run: () => setOverlay("refs") }));
    const fileEntries: PaletteEntry[] = files().map((f) => ({
      kind: "file",
      label: f,
      run: () => {
        setNavFile(f);
        setOverlay("blame");
      },
    }));
    return [...repos, ...actions, ...worktrees, ...branches, ...fileEntries];
  };

  const goPrimary = (v: PrimaryView) => {
    setOverlay(null);
    setView(v);
    setDrawerOpen(false);
  };

  // Single-click a sidebar ref → show it in All Commits, tip commit selected.
  const showRef = (ref: RefInfo) => {
    setSelectedCommits([ref.target]);
    setPrimaryCommit(ref.target);
    goPrimary("commits");
  };

  // Advanced views in the toolbar "More" menu.
  const overflowItems = (): OverflowItem[] => [
    { key: "refs", label: "Refs / Pull Requests" },
    { key: "blame", label: "Blame" },
    { key: "history", label: "File History" },
    { key: "worktrees", label: "Manage Worktrees" },
    { key: "submodules", label: "Submodules" },
    { key: "stashes", label: "Stashes" },
    { key: "reflog", label: "Reflog" },
    { key: "bisect", label: "Bisect" },
    { key: "flow", label: "git-flow" },
    { key: "lfs", label: "Git LFS" },
    { key: "commands", label: "Custom Commands" },
    { key: "ai", label: "✨ AI Assistant" },
    { key: "notifications", label: "Notifications", badge: unread() || undefined },
  ];

  const openBranchMenu = (branch: string, at: { x: number; y: number }) =>
    setBranchMenu({
      branch,
      isCurrent: branch === currentBranchName(),
      x: at.x,
      y: at.y,
    });

  const openCommitMenu = (oid: string, summary: string, at: { x: number; y: number }) =>
    setCommitMenu({ oid, summary, x: at.x, y: at.y });

  const openTagMenu = (tag: string, at: { x: number; y: number }) =>
    setTagMenu({ tag, x: at.x, y: at.y });

  // Explain a single commit via the AI overlay (commit-menu / tag-menu target).
  const explainCommit = (oid: string) =>
    openExplain({ kind: "commit", oid }, `Explain ${oid.slice(0, 8)}`, oid.slice(0, 8));

  // Create a branch from the modal's start point + name.
  const doCreateBranch = async () => {
    const repo = repoId();
    const startPoint = newBranchFrom();
    const name = newBranchName().trim();
    if (!repo || !startPoint || !name) return;
    const r = await createBranchAt(repo, name, startPoint);
    setNewBranchFrom(null);
    setNewBranchName("");
    if (r.ok) {
      setErr(null);
      setOpNotice(r.message);
    } else {
      setErr(r.message);
    }
    refresh();
  };

  // Surface a branch/file ActionResult as a toast or error, then refresh.
  const showResult = (r: { ok: boolean; message: string } | null) => {
    if (!r) return;
    if (r.ok) {
      setErr(null);
      setOpNotice(r.message);
    } else {
      setErr(r.message);
    }
    refresh();
  };

  // Open the push dialog for any ref (branch or tag). All push operations in
  // Lady route through here so the user can confirm the remote and opt into a
  // force push.
  const openPushDialog = (state: Omit<PushDialogState, "onSuccess" | "onError"> & { onSuccess?: () => void; onError?: (msg: string) => void }) => {
    setPushDialog({
      ...state,
      onSuccess: () => {
        setPushDialog(null);
        state.onSuccess?.();
        refresh();
      },
      onError: (msg) => {
        setPushDialog(null);
        state.onError?.(msg);
        setErr(msg);
      },
    });
  };

  // Open the generalized name prompt (Rename, New Tag, …).
  const openPrompt = (spec: PromptSpec) => {
    setPromptValue(spec.initial ?? "");
    setPromptSpec(spec);
  };
  const submitPrompt = () => {
    const spec = promptSpec();
    const v = promptValue().trim();
    if (!spec || !v) return;
    setPromptSpec(null);
    setPromptValue("");
    spec.onSubmit(v);
  };

  // Pick a directory and create a worktree for `branch` there.
  const pickWorktreeDir = async (branch: string) => {
    const repo = repoId();
    if (!repo) return;
    const dir = await openDialog({ directory: true, title: `Worktree directory for ${branch}` });
    if (typeof dir !== "string") return;
    showResult(await addWorktreeFor(repo, branch, dir));
  };

  // Sidebar branch-row keyboard shortcuts (⇧⌘B / ⇧⌘G / ⌫).
  const onBranchKey = async (branch: string, action: "new-branch" | "new-tag" | "delete") => {
    const repo = repoId();
    if (!repo) return;
    if (action === "new-branch") {
      setNewBranchName("");
      setNewBranchFrom(branch);
    } else if (action === "new-tag") {
      openPrompt({
        title: `New tag at ${branch}`,
        placeholder: "tag-name",
        submitLabel: "Create Tag",
        onSubmit: async (name) => showResult(await createTagAt(repo, name, branch)),
      });
    } else {
      showResult(await deleteBranch(repo, branch));
    }
  };

  // Generic horizontal drag: feed each pointermove delta to `apply`.
  const hDrag = (e: PointerEvent, apply: (dx: number) => void) => {
    e.preventDefault();
    const startX = e.clientX;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => apply(ev.clientX - startX);
    const up = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId);
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };
  const startSidebarDrag = (e: PointerEvent) => {
    const startW = sidebarWidth();
    hDrag(e, (dx) => setSidebarWidth(Math.max(180, Math.min(startW + dx, window.innerWidth - 320))));
  };
  // Settings dialog: a right-edge handle; dragging right grows the dialog.
  const startSettingsDrag = (e: PointerEvent) => {
    const startW = settingsWidth();
    hDrag(e, (dx) => setSettingsWidth(Math.max(420, Math.min(startW + dx * 2, window.innerWidth - 48))));
  };

  // Open the AI "Explain changes" overlay for any ai_explain target.
  const openExplain = (target: Record<string, unknown>, title: string, subtitle?: string) =>
    setExplainSpec({ target, title, subtitle });

  // Explain a branch's changes vs the mainline (main/master), else its tip.
  const explainBranch = (branch: string) => {
    const names = refs().filter((r) => r.kind === "Branch").map((r) => r.name);
    const base = ["main", "master"].find((b) => names.includes(b) && b !== branch);
    if (base) {
      openExplain({ kind: "branch_range", base, head: branch }, `Explain ${branch}`, `${base}..${branch}`);
    } else {
      // No mainline to diff against (or branch IS it): explain its tip commit.
      openExplain({ kind: "branch_range", base: `${branch}^`, head: branch }, `Explain ${branch}`, `latest commit`);
    }
  };

  // Open the Blame / File-History overlay focused on a path (from file menus).
  const openBlameFor = (path: string) => {
    setNavFile(path);
    setOverlay("blame");
  };
  const openHistoryFor = (path: string) => {
    setNavFile(path);
    setOverlay("history");
  };

  // Double-click a sidebar branch → check it out (force-fallback handled inside).
  const checkoutByName = async (branch: string) => {
    const repo = repoId();
    if (!repo) return;
    const r = await checkoutBranch(repo, branch);
    if (r.ok) {
      setErr(null);
      setOpNotice(r.message);
    } else {
      setErr(r.message);
    }
    refresh();
  };

  const bar = {
    margin: 0,
    padding: "4px 16px",
    "font-size": "12px",
    "flex-shrink": 0,
  } as const;

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column", overflow: "hidden" }}>
      <Toolbar
        repoId={repoId()}
        repoName={repoName()}
        currentBranch={currentBranchName()}
        refreshNonce={refreshNonce()}
        onChanged={refresh}
        overflowItems={overflowItems()}
        onOverflow={(k) => setOverlay(k as Overlay)}
        onQuickLaunch={() => setPaletteOpen(true)}
        onToggleSidebar={() => setDrawerOpen((v) => !v)}
        onAddRepo={() => repoPicker?.()}
        onBranchMenu={(at) => {
          const b = currentBranchName();
          if (b) openBranchMenu(b, at);
        }}
        onSettings={() => setOverlay("settings")}
        onPush={() => {
          const b = currentBranchName();
          if (b) openPushDialog({ repo: repoId()!, refspec: b, remote: "origin", isTag: false });
        }}
      />

      {/* Licensing gate: blocks the UI when the trial has expired (ADR-0007). */}
      <Show when={license()?.kind === "Expired"}>
        <LicenseGate onActivated={setLicense} />
      </Show>

      {/* Repo tab strip (34px) */}
      <RepoBar
        active={repoId()}
        onActiveChange={setActive}
        apiRef={(api) => {
          openRepoPath = api.open;
          repoPicker = api.pick;
        }}
        onRecents={setRecents}
      />

      {/* Thin status bars */}
      <Show when={license()?.kind === "Trial"}>
        <div role="status" style={{ ...bar, background: "var(--warning-bg)", color: "var(--warning)" }}>
          Trial — {(license() as { days_left: number }).days_left} day
          {(license() as { days_left: number }).days_left === 1 ? "" : "s"} left
        </div>
      </Show>
      <Show when={err()}>
        <p role="alert" style={{ ...bar, color: "var(--error)" }}>{err()}</p>
      </Show>
      <Show when={opConflicts().length > 0}>
        <div role="alert" style={{ ...bar, border: "1px solid var(--warning-border)", background: "var(--warning-bg)" }}>
          <span style={{ "font-weight": 700 }}>Conflicts: </span>
          <span>{opConflicts().join(", ")}</span>
          <button style={{ "margin-left": "0.5rem" }} onClick={() => setOverlay("conflicts")}>Resolve</button>
          <button style={{ "margin-left": "0.5rem" }} onClick={abortSequencer}>Abort</button>
        </div>
      </Show>
      <Show when={acctSuggestion()}>
        <div role="status" style={{ ...bar, border: "1px solid var(--border)", background: "var(--surface-2)" }}>
          <span>
            Use your <strong>{acctSuggestion()!.account.login}</strong> GitHub account for this repo?{" "}
            <span style={{ color: "var(--tx3)" }}>{acctSuggestion()!.reason}</span>
          </span>
          <button style={{ "margin-left": "0.5rem" }} onClick={acceptSuggestion}>Use it</button>
          <button style={{ "margin-left": "0.5rem" }} onClick={dismissSuggestion}>Not now</button>
        </div>
      </Show>

      <Show when={updateAvail()}>
        <div role="status" style={{ ...bar, border: "1px solid var(--border)", background: "var(--surface-2)" }}>
          <span>
            Update available: <strong>v{updateAvail()!.version}</strong> (you have v{updateAvail()!.current})
          </span>
          <button style={{ "margin-left": "0.5rem" }} onClick={installLaunchUpdate} disabled={updateBusy()}>
            {updateBusy() ? "Installing…" : "Update & restart"}
          </button>
          <button style={{ "margin-left": "0.5rem" }} onClick={() => setUpdateAvail(null)} disabled={updateBusy()}>Later</button>
        </div>
      </Show>

      {/* Body: sidebar + main */}
      <Show
        when={active()}
        fallback={
          <div style={{ flex: "1", display: "flex", "align-items": "center", "justify-content": "center", color: "var(--tx3)" }}>
            Open a repository to begin.
          </div>
        }
      >
        <div style={{ flex: "1", display: "flex", "flex-direction": isNarrow() ? "column" : "row", "min-height": "0", overflow: "hidden" }}>
          {/* Side-by-side: the sidebar lives inline. On narrow it moves into the
              off-canvas drawer below (hamburger in the toolbar opens it). */}
          <Show when={!isNarrow()}>
            <Sidebar
              repoId={repoId()}
              repoName={repoName()}
              changeCount={changeCount()}
              refreshNonce={refreshNonce()}
              view={view()}
              onView={goPrimary}
              refs={refs()}
              onBranchMenu={openBranchMenu}
              onTagMenu={openTagMenu}
              onCheckout={checkoutByName}
              onSelectRef={showRef}
              onBranchKey={onBranchKey}
              onOpenStashes={() => setOverlay("stashes")}
              onOpenWorktree={(path) => openRepoPath?.(path)}
              onManageWorktrees={() => setOverlay("worktrees")}
            />

            {/* Drag handle: resize the sidebar (col-resize). Hidden on touch. */}
            <Show when={!hideResizers()}>
              <div
                onPointerDown={startSidebarDrag}
                title="Drag to resize the sidebar"
                style={{ "flex-shrink": 0, width: "6px", cursor: "col-resize", "margin-left": "-3px", "z-index": 5, "touch-action": "none" }}
              />
            </Show>
          </Show>

          <div role="main" aria-label={overlay() ?? view()} style={{ flex: "1", "min-width": "0", overflow: "hidden", background: "var(--bg)" }}>
            {/* Overlay (advanced view) takes precedence over the primary view —
                except Settings, which floats as a dialog over the primary view. */}
            <Show when={overlay() === null || overlay() === "settings"} fallback={renderOverlay()}>
              <Show when={view() === "changes"}>
                <ChangesView
                  repoId={repoId()!}
                  refreshNonce={refreshNonce()}
                  onChanged={refresh}
                  onResult={showResult}
                  onPush={() => {
                    const b = currentBranchName();
                    if (b) openPushDialog({ repo: repoId()!, refspec: b, remote: "origin", isTag: false });
                  }}
                  onOpenBlame={openBlameFor}
                  onOpenHistory={openHistoryFor}
                  onExplain={openExplain}
                />
              </Show>
              <Show when={view() === "commits"}>
                <AllCommitsView
                  repoId={repoId()!}
                  refs={refs()}
                  selected={selectedCommits()}
                  primary={primaryCommit() ?? undefined}
                  onSelectionChange={(oids, primary) => {
                    setSelectedCommits(oids);
                    setPrimaryCommit(primary);
                  }}
                  sig={selectedSig()}
                  onCherryPick={() => runCommitAction("cherry_pick", "Cherry-pick")}
                  onRevert={() => runCommitAction("revert", "Revert")}
                  onRebaseInteractive={(oid) => setRebaseFrom(oid)}
                  onRecompose={(oid) => setRecomposeFrom(oid)}
                  onCommitMenu={openCommitMenu}
                />
              </Show>
            </Show>
          </div>

          {/* Off-canvas drawer (narrow only): a backdrop + sliding sidebar panel.
              The toolbar hamburger toggles it; tapping the backdrop or navigating
              closes it. */}
          <Show when={isNarrow() && drawerOpen()}>
            <div
              onClick={() => setDrawerOpen(false)}
              style={{ position: "fixed", inset: "0", background: "rgba(0,0,0,0.45)", "z-index": 900 }}
            />
            <div
              class="scroll-thin"
              style={{
                position: "fixed",
                top: "0",
                bottom: "0",
                left: "0",
                "z-index": 901,
                width: "min(84vw, 320px)",
                "overflow-y": "auto",
                background: "var(--panel)",
                "border-right": "1px solid var(--bd)",
                "padding-left": "env(safe-area-inset-left, 0px)",
                animation: "drawerIn 0.22s ease",
              }}
            >
              <Sidebar
                fullWidth
                repoId={repoId()}
                repoName={repoName()}
                changeCount={changeCount()}
                refreshNonce={refreshNonce()}
                view={view()}
                onView={goPrimary}
                refs={refs()}
                onBranchMenu={openBranchMenu}
                onTagMenu={openTagMenu}
                onCheckout={checkoutByName}
                onSelectRef={showRef}
                onBranchKey={onBranchKey}
                onOpenStashes={() => { setDrawerOpen(false); setOverlay("stashes"); }}
                onOpenWorktree={(path) => { setDrawerOpen(false); openRepoPath?.(path); }}
                onManageWorktrees={() => { setDrawerOpen(false); setOverlay("worktrees"); }}
              />
            </div>
          </Show>
        </div>
      </Show>

      {/* Branch context menu (sidebar ⋯ / right-click, toolbar Branch button) */}
      <Show when={branchMenu() && active()}>
        <BranchMenu
          repoId={repoId()!}
          currentBranch={currentBranchName()}
          refs={refs()}
          state={branchMenu()!}
          onClose={() => setBranchMenu(null)}
          onResult={showResult}
          onCreateBranch={(startPoint) => {
            setNewBranchName("");
            setNewBranchFrom(startPoint);
          }}
          onPrompt={openPrompt}
          onWorktree={pickWorktreeDir}
          onAiExplain={explainBranch}
          onCreatePr={() => setOverlay("refs")}
          onPush={(branch) => openPushDialog({ repo: repoId()!, refspec: branch, remote: "origin", isTag: false })}
        />
      </Show>

      {/* Commit context menu (right-click a commit row in All Commits) */}
      <Show when={commitMenu() && active()}>
        <CommitMenu
          repoId={repoId()!}
          currentBranch={currentBranchName()}
          refs={refs()}
          state={commitMenu()!}
          onClose={() => setCommitMenu(null)}
          onResult={showResult}
          onRebaseComplete={onRebaseComplete}
          onCreateBranch={(startPoint) => {
            setNewBranchName("");
            setNewBranchFrom(startPoint);
          }}
          onPrompt={openPrompt}
          onWorktree={pickWorktreeDir}
          onAiExplain={explainCommit}
          onRecompose={(oid) => setRecomposeFrom(oid)}
          onCreatePr={() => setOverlay("refs")}
          onPush={(branch) => openPushDialog({ repo: repoId()!, refspec: branch, remote: "origin", isTag: false })}
        />
      </Show>

      {/* Tag context menu (right-click a tag in the sidebar Tags panel) */}
      <Show when={tagMenu() && active()}>
        <TagMenu
          repoId={repoId()!}
          currentBranch={currentBranchName()}
          refs={refs()}
          defaultBranch={defaultBranch()}
          state={tagMenu()!}
          onClose={() => setTagMenu(null)}
          onResult={showResult}
          onMutate={refresh}
          onCreateBranch={(startPoint) => {
            setNewBranchName("");
            setNewBranchFrom(startPoint);
          }}
          onAiExplain={explainCommit}
          onPush={(tag) => openPushDialog({ repo: repoId()!, refspec: `refs/tags/${tag}`, remote: "origin", isTag: true })}
          onInteractiveRebase={(branch, ontoTag) => {
            // Find the tag's target oid; interactive rebase works from the
            // common-ancestor commit between branch and the tag's commit.
            const tagRef = refs().find((r) => r.kind === "Tag" && r.name === ontoTag);
            if (tagRef) setRebaseFrom(tagRef.target);
          }}
        />
      </Show>

      {/* New-branch name modal (Create Branch Here…) */}
      <Show when={newBranchFrom()}>
        <div
          style={{ position: "fixed", inset: "0", background: "rgba(0,0,0,0.35)", display: "flex", "align-items": "flex-start", "justify-content": "center", "padding-top": "18vh", "z-index": "1000" }}
          onClick={() => setNewBranchFrom(null)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{ width: "min(420px, 90vw)", background: "var(--pill)", border: "1px solid var(--bd)", "border-radius": "9px", padding: "16px", "box-shadow": "0 14px 38px rgba(0,0,0,0.45)", display: "flex", "flex-direction": "column", gap: "10px" }}
          >
            <div style={{ "font-size": "13px", color: "var(--tx2)" }}>
              New branch from <span style={{ "font-family": "ui-monospace, monospace", color: "var(--tx)" }}>{newBranchFrom()}</span>
            </div>
            <input
              ref={(el) => queueMicrotask(() => el.focus())}
              value={newBranchName()}
              onInput={(e) => setNewBranchName(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") doCreateBranch();
                if (e.key === "Escape") setNewBranchFrom(null);
              }}
              placeholder="new-branch-name"
              style={{ background: "var(--input)", border: "1px solid var(--bd)", "border-radius": "7px", color: "var(--tx)", "font-size": "13px", padding: "9px 12px" }}
            />
            <div style={{ display: "flex", "justify-content": "flex-end", gap: "8px" }}>
              <button onClick={() => setNewBranchFrom(null)} style={{ border: "1px solid var(--bd)", background: "var(--btn)", color: "var(--tx)", "border-radius": "7px", padding: "7px 16px", cursor: "pointer", "font-size": "12.5px" }}>
                Cancel
              </button>
              <button
                onClick={doCreateBranch}
                disabled={!newBranchName().trim()}
                style={{ border: "none", background: newBranchName().trim() ? "var(--accent)" : "var(--btn)", color: newBranchName().trim() ? "var(--on-accent-strong)" : "var(--tx3)", "border-radius": "7px", padding: "7px 16px", cursor: newBranchName().trim() ? "pointer" : "not-allowed", "font-size": "12.5px", "font-weight": 600 }}
              >
                Create Branch
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Generalized name prompt (Rename, New Tag, …) */}
      <Show when={promptSpec()}>
        <div
          style={{ position: "fixed", inset: "0", background: "rgba(0,0,0,0.35)", display: "flex", "align-items": "flex-start", "justify-content": "center", "padding-top": "18vh", "z-index": "1000" }}
          onClick={() => setPromptSpec(null)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{ width: "min(420px, 90vw)", background: "var(--pill)", border: "1px solid var(--bd)", "border-radius": "9px", padding: "16px", "box-shadow": "0 14px 38px rgba(0,0,0,0.45)", display: "flex", "flex-direction": "column", gap: "10px" }}
          >
            <div style={{ "font-size": "13px", color: "var(--tx2)" }}>{promptSpec()!.title}</div>
            <input
              ref={(el) => queueMicrotask(() => el.focus())}
              value={promptValue()}
              onInput={(e) => setPromptValue(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submitPrompt();
                if (e.key === "Escape") setPromptSpec(null);
              }}
              placeholder={promptSpec()!.placeholder}
              style={{ background: "var(--input)", border: "1px solid var(--bd)", "border-radius": "7px", color: "var(--tx)", "font-size": "13px", padding: "9px 12px" }}
            />
            <div style={{ display: "flex", "justify-content": "flex-end", gap: "8px" }}>
              <button onClick={() => setPromptSpec(null)} style={{ border: "1px solid var(--bd)", background: "var(--btn)", color: "var(--tx)", "border-radius": "7px", padding: "7px 16px", cursor: "pointer", "font-size": "12.5px" }}>
                Cancel
              </button>
              <button
                onClick={submitPrompt}
                disabled={!promptValue().trim()}
                style={{ border: "none", background: promptValue().trim() ? "var(--accent)" : "var(--btn)", color: promptValue().trim() ? "var(--on-accent-strong)" : "var(--tx3)", "border-radius": "7px", padding: "7px 16px", cursor: promptValue().trim() ? "pointer" : "not-allowed", "font-size": "12.5px", "font-weight": 600 }}
              >
                {promptSpec()!.submitLabel}
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Global settings dialog — opens with or without a repo (repo-specific
          sections appear only when a repo is active). */}
      <Show when={overlay() === "settings"}>
        <div
          style={{ position: "fixed", inset: "0", background: "rgba(0,0,0,0.45)", display: "flex", "align-items": "center", "justify-content": "center", "z-index": "1100" }}
          onClick={() => setOverlay(null)}
        >
          <div
            role="dialog"
            aria-label="Settings"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              if (e.key === "Escape") setOverlay(null);
            }}
            style={{ position: "relative", width: `min(${settingsWidth()}px, 94vw)`, height: "min(860px, 90vh)", background: "var(--panel)", border: "1px solid var(--bd)", "border-radius": "12px", "box-shadow": "0 24px 70px rgba(0,0,0,0.5)", display: "flex", "flex-direction": "column", overflow: "hidden" }}
          >
            {/* Right-edge drag handle to widen / narrow the dialog. */}
            <div
              onPointerDown={startSettingsDrag}
              title="Drag to resize"
              style={{ position: "absolute", top: 0, right: "-4px", width: "12px", height: "100%", cursor: "col-resize", "touch-action": "none", "z-index": 2 }}
            />
            <div style={{ "flex-shrink": 0, display: "flex", "align-items": "center", gap: "8px", padding: "12px 16px", "border-bottom": "1px solid var(--bd)", background: "var(--toolbar)" }}>
              <span style={{ "font-size": "13px", "font-weight": 600, color: "var(--tx)" }}>Settings</span>
              <span style={{ flex: "1" }} />
              <button
                aria-label="Close settings"
                onClick={() => setOverlay(null)}
                style={{ border: "1px solid var(--bd)", background: "var(--btn)", color: "var(--tx)", "border-radius": "7px", padding: "5px 12px", cursor: "pointer", "font-size": "12.5px" }}
              >
                Close
              </button>
            </div>
            <div style={{ flex: "1", "min-height": "0", overflow: "hidden" }}>
              <SettingsView repoId={repoId()} />
            </div>
          </div>
        </div>
      </Show>

      {/* Push confirmation dialog — shown for every push operation. */}
      <Show when={pushDialog() && active()}>
        <PushDialog
          state={pushDialog()!}
          onClose={() => setPushDialog(null)}
        />
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

      {/* AI "Explain changes" overlay (file / branch / hunk) */}
      <Show when={explainSpec() && active()}>
        <ExplainPanel
          repoId={repoId()!}
          target={explainSpec()!.target}
          title={explainSpec()!.title}
          subtitle={explainSpec()!.subtitle}
          onClose={() => setExplainSpec(null)}
        />
      </Show>

      {/* AI recompose overlay (Plan 3) */}
      <Show when={recomposeFrom() && active()}>
        <RecomposeView
          repoId={repoId()!}
          fromOid={recomposeFrom()!}
          onClose={() => setRecomposeFrom(null)}
          onComplete={refresh}
        />
      </Show>

      {/* Transient status toast (auto-fades; conflicts/errors stay as bars). */}
      <Show when={opNotice()}>
        <div
          role="status"
          onClick={() => setOpNotice(null)}
          style={{
            position: "fixed",
            bottom: "calc(20px + env(safe-area-inset-bottom, 0px))",
            left: "50%",
            transform: "translateX(-50%)",
            "z-index": "1200",
            "max-width": "min(560px, 90vw)",
            display: "flex",
            "align-items": "center",
            gap: "8px",
            padding: "9px 16px",
            background: "var(--pill)",
            border: "1px solid var(--bd)",
            "border-radius": "9px",
            "box-shadow": "0 12px 32px rgba(0,0,0,0.4)",
            color: "var(--tx)",
            "font-size": "12.5px",
            cursor: "pointer",
            opacity: toastLeaving() ? 0 : 1,
            transition: "opacity 0.35s ease",
            animation: "toastIn 0.22s ease",
          }}
        >
          <span style={{ color: "var(--success)" }}>✓</span>
          {opNotice()}
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

  // ── Overlay (advanced) view rendering ───────────────────────────────────────
  function renderOverlay() {
    const id = repoId();
    if (!id) return null;
    return (
      <>
        <Show when={overlay() === "refs"}>
          <RefsView repoId={id} refs={refs()} onChanged={refresh} onInteractiveRebase={(oid) => setRebaseFrom(oid)} />
        </Show>
        <Show when={overlay() === "blame"}>
          <BlameView repoId={id} initialPath={navFile()} />
        </Show>
        <Show when={overlay() === "history"}>
          <FileHistory repoId={id} initialPath={navFile()} />
        </Show>
        <Show when={overlay() === "worktrees"}>
          <WorktreesView repoId={id} refreshNonce={refreshNonce()} onChanged={refresh} onOpen={(path) => openRepoPath?.(path)} />
        </Show>
        <Show when={overlay() === "reflog"}>
          <ReflogView repoId={id} refreshNonce={refreshNonce()} onChanged={refresh} />
        </Show>
        <Show when={overlay() === "bisect"}>
          <BisectView repoId={id} refreshNonce={refreshNonce()} onChanged={refresh} />
        </Show>
        <Show when={overlay() === "commands"}>
          <CustomCommandsView repoId={id} refs={refs()} files={files()} />
        </Show>
        <Show when={overlay() === "lfs"}>
          <LfsView repoId={id} refreshNonce={refreshNonce()} onChanged={refresh} />
        </Show>
        <Show when={overlay() === "flow"}>
          <GitFlowView repoId={id} refreshNonce={refreshNonce()} onChanged={refresh} />
        </Show>
        <Show when={overlay() === "submodules"}>
          <SubmodulesView repoId={id} repoPath={active()!.path} refreshNonce={refreshNonce()} onChanged={refresh} onOpen={(path) => openRepoPath?.(path)} />
        </Show>
        <Show when={overlay() === "stashes"}>
          <StashView repoId={id} refreshNonce={refreshNonce()} onChanged={refresh} />
        </Show>
        <Show when={overlay() === "ai"}>
          <AiView repoId={id} onChanged={refresh} />
        </Show>
        {/* Settings renders as a centered dialog (see below), not a pane. */}
        <Show when={overlay() === "notifications"}>
          <NotificationsView refreshNonce={refreshNonce()} onUnread={setUnread} />
        </Show>
        <Show when={overlay() === "conflicts"}>
          <ConflictResolver
            repoId={id}
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
      </>
    );
  }
};

export default App;
