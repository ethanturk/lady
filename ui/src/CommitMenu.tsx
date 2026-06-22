import { createSignal, onMount } from "solid-js";
import type { Component } from "solid-js";
import type { RebaseOutcome, RefInfo, RepoId } from "./commands";
import ContextMenu from "./ContextMenu";
import type { MenuEntry } from "./ContextMenu";
import type { PromptSpec } from "./BranchMenu";
import {
  ActionResult,
  branchUpstream,
  copyBranchName,
  createAnnotatedTagAt,
  createTagAt,
  deleteBranch,
  pullCurrent,
  pushBranch,
  renameBranch,
  setTracking,
} from "./branchActions";
import {
  checkoutCommit,
  copyCommitSha,
  copyRemoteLink,
  createBranchAtCommit,
  dropCommit,
  moveCommitDown,
  resetTo,
  revertCommit,
  rewordCommit,
} from "./commitActions";
import { deleteRemoteBranch } from "./tagActions";

export interface CommitMenuState {
  oid: string;
  /** The commit's summary line, used to pre-fill the reword prompt. */
  summary: string;
  x: number;
  y: number;
}

interface CommitMenuProps {
  repoId: RepoId;
  /** Name of the current (HEAD) branch; null when detached. Gates rewrite ops. */
  currentBranch: string | null;
  /** All refs, for branch-tip detection and the Tracking submenu. */
  refs: RefInfo[];
  state: CommitMenuState;
  onClose: () => void;
  /** Result (or null if cancelled) — caller shows + refreshes. */
  onResult: (r: ActionResult | null) => void;
  /** A rebase outcome (drop / reword / move) — caller routes conflicts to the resolver. */
  onRebaseComplete: (outcome: RebaseOutcome) => void;
  /** Open the "new branch from <startPoint>" name modal. */
  onCreateBranch: (startPoint: string) => void;
  /** Open a generalized name prompt (New Tag, Reword, Rename). */
  onPrompt: (spec: PromptSpec) => void;
  /** Pick a directory then create a worktree at the given start point. */
  onWorktree: (startPoint: string) => void;
  /** Open the AI view to explain this commit. */
  onAiExplain: (oid: string) => void;
  /** Open the AI recompose flow from this commit. */
  onRecompose: (oid: string) => void;
  /** Open the Pull-Request creation flow for a branch. */
  onCreatePr: (branch: string) => void;
}

/**
 * The commit-graph context menu (right-click a commit row). A Fork/GitKraken-
 * style menu: checkout, branch/tag/worktree creation, reset (soft/mixed/hard),
 * history rewrite (reword / drop / move down) via the interactive-rebase engine,
 * revert, AI recompose/explain, and copy SHA/link. When the commit is a local
 * branch tip, that branch's actions (pull/push/tracking/PR/rename/delete/copy)
 * are appended. Rewrite items are disabled on a detached HEAD.
 */
const CommitMenu: Component<CommitMenuProps> = (props) => {
  const oid = () => props.state.oid;
  const cur = () => props.currentBranch ?? "current";
  const canRewrite = () => props.currentBranch != null;

  // A local branch whose tip is this commit (first match), for the tip section.
  const tipBranch = (): string | null =>
    props.refs.find((r) => r.kind === "Branch" && r.target === oid())?.name ?? null;

  const [upstream, setUpstream] = createSignal<string | null>(null);
  onMount(() => {
    const b = tipBranch();
    if (b) branchUpstream(props.repoId, b).then(setUpstream);
  });

  const result = (r: ActionResult | null) => props.onResult(r);

  // Run a rewrite helper that returns a RebaseOutcome, routing errors to a toast.
  const runRewrite = async (fn: () => Promise<RebaseOutcome>) => {
    try {
      props.onRebaseComplete(await fn());
    } catch (e) {
      result({ ok: false, message: String(e) });
    }
  };

  const resetSubmenu = (): MenuEntry[] => [
    { label: "Soft (keep changes staged)", run: async () => result(await resetTo(props.repoId, oid(), "Soft")) },
    { label: "Mixed (unstage changes)", run: async () => result(await resetTo(props.repoId, oid(), "Mixed")) },
    { label: "Hard (discard changes)", danger: true, run: async () => result(await resetTo(props.repoId, oid(), "Hard")) },
  ];

  const remoteRefs = (): string[] =>
    props.refs.filter((r) => r.kind === "Remote").map((r) => r.name);

  const trackingSubmenu = (branch: string): MenuEntry[] => {
    const items: MenuEntry[] = remoteRefs().map((rref) => ({
      label: rref,
      run: async () => result(await setTracking(props.repoId, branch, rref)),
    }));
    if (upstream()) {
      if (items.length > 0) items.push("divider");
      items.push({ label: "Unset upstream", run: async () => result(await setTracking(props.repoId, branch, null)) });
    }
    if (items.length === 0) items.push({ label: "(no remotes)", disabled: true });
    return items;
  };

  const items = (): MenuEntry[] => {
    const list: MenuEntry[] = [
      { label: "Checkout Commit", run: async () => result(await checkoutCommit(props.repoId, oid())) },
      {
        label: "New Branch Here…",
        shortcut: "⇧⌘B",
        run: () => props.onCreateBranch(oid()),
      },
      { label: "Checkout as Worktree…", run: () => props.onWorktree(oid()) },
      "divider",
      {
        label: `Reset ${cur()} to Here`,
        disabled: !canRewrite(),
        submenu: resetSubmenu(),
      },
      {
        label: "Edit Commit Message…",
        disabled: !canRewrite(),
        run: () =>
          props.onPrompt({
            title: `Reword ${oid().slice(0, 8)}`,
            placeholder: "new commit message",
            initial: props.state.summary,
            submitLabel: "Reword",
            onSubmit: (msg) => runRewrite(() => rewordCommit(props.repoId, oid(), msg)),
          }),
      },
      { label: "Revert Commit", run: async () => result(await revertCommit(props.repoId, oid())) },
      {
        label: "Drop Commit",
        danger: true,
        disabled: !canRewrite(),
        run: () => runRewrite(() => dropCommit(props.repoId, oid())),
      },
      {
        label: "Move Commit Down",
        disabled: !canRewrite(),
        run: () => runRewrite(() => moveCommitDown(props.repoId, oid())),
      },
      "divider",
      { label: "✨ Recompose with AI", run: () => props.onRecompose(oid()) },
      { label: "Explain Commit", run: () => props.onAiExplain(oid()) },
      "divider",
      {
        label: "New Tag Here…",
        shortcut: "⇧⌘G",
        run: () =>
          props.onPrompt({
            title: `New tag at ${oid().slice(0, 8)}`,
            placeholder: "tag-name",
            submitLabel: "Create Tag",
            onSubmit: async (name) => result(await createTagAt(props.repoId, name, oid())),
          }),
      },
      {
        label: "New Annotated Tag Here…",
        run: () =>
          props.onPrompt({
            title: `Annotated tag at ${oid().slice(0, 8)}`,
            placeholder: "tag-name",
            submitLabel: "Next: message",
            onSubmit: (name) =>
              props.onPrompt({
                title: `Message for ${name}`,
                placeholder: "tag message",
                submitLabel: "Create Tag",
                onSubmit: async (message) => result(await createAnnotatedTagAt(props.repoId, name, oid(), message)),
              }),
          }),
      },
      "divider",
      { label: "Copy Commit SHA", shortcut: "⌘C", run: async () => result(await copyCommitSha(oid())) },
      { label: "Copy Link to Commit", run: async () => result(await copyRemoteLink(props.repoId, { kind: "Commit", value: oid() })) },
    ];

    // Branch-tip section: this commit is a local branch's tip.
    const b = tipBranch();
    if (b) {
      const remote = upstream()?.split("/")[0] ?? "origin";
      list.push(
        "divider",
        { label: `— branch ${b} —`, disabled: true },
        { label: `Pull '${upstream() ?? "upstream"}'`, disabled: b !== props.currentBranch || !upstream(), run: async () => result(await pullCurrent(props.repoId)) },
        { label: `Push to '${remote}'…`, run: async () => result(await pushBranch(props.repoId, b, !upstream())) },
        { label: "Set Upstream (Tracking)", submenu: trackingSubmenu(b) },
        { label: `Create Pull Request on '${remote}'…`, run: () => props.onCreatePr(b) },
        {
          label: "Rename Branch…",
          run: () =>
            props.onPrompt({
              title: `Rename ${b}`,
              placeholder: "new-branch-name",
              initial: b,
              submitLabel: "Rename",
              onSubmit: async (name) => result(await renameBranch(props.repoId, b, name)),
            }),
        },
        { label: `Delete ${b}`, danger: true, disabled: b === props.currentBranch, run: async () => result(await deleteBranch(props.repoId, b)) },
        { label: `Delete ${b} from '${remote}'`, danger: true, run: async () => result(await deleteRemoteBranch(props.repoId, remote, b)) },
        { label: "Copy Branch Name", run: async () => result(await copyBranchName(b)) },
        { label: "Copy Link to Branch", run: async () => result(await copyRemoteLink(props.repoId, { kind: "Branch", value: b })) },
      );
    }
    return list;
  };

  return <ContextMenu x={props.state.x} y={props.state.y} items={items()} onClose={props.onClose} />;
};

export default CommitMenu;
