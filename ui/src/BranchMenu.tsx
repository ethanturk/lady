import { createSignal, onMount } from "solid-js";
import type { Component } from "solid-js";
import type { RefInfo, RepoId } from "./commands";
import ContextMenu from "./ContextMenu";
import type { MenuEntry } from "./ContextMenu";
import {
  ActionResult,
  branchUpstream,
  checkoutBranch,
  copyBranchName,
  createTagAt,
  deleteBranch,
  fastForwardBranch,
  mergeIntoCurrent,
  pullCurrent,
  rebaseCurrentOnto,
  renameBranch,
  setTracking,
} from "./branchActions";

export interface BranchMenuState {
  branch: string;
  isCurrent: boolean;
  x: number;
  y: number;
}

/** A generalized in-app name prompt (window.prompt is unsupported in the webview). */
export interface PromptSpec {
  title: string;
  placeholder: string;
  initial?: string;
  submitLabel: string;
  onSubmit: (value: string) => void;
}

interface BranchMenuProps {
  repoId: RepoId;
  /** Name of the current (HEAD) branch, for the merge/rebase labels. */
  currentBranch: string | null;
  /** All refs, for the Tracking submenu (remote refs become tracking targets). */
  refs: RefInfo[];
  state: BranchMenuState;
  onClose: () => void;
  /** Result (or null if the action was cancelled) — caller shows + refreshes. */
  onResult: (r: ActionResult | null) => void;
  /** Open the "new branch from <startPoint>" name modal (avoids window.prompt). */
  onCreateBranch: (startPoint: string) => void;
  /** Open a generalized name prompt (Rename, New Tag). */
  onPrompt: (spec: PromptSpec) => void;
  /** Pick a directory then create a worktree for the branch there. */
  onWorktree: (branch: string) => void;
  /** Open the AI view to explain this branch. */
  onAiExplain: (branch: string) => void;
  /** Open the pull-request creation flow (Refs view) for this branch. */
  onCreatePr: (branch: string) => void;
  /** Open the push confirmation dialog for this branch. */
  onPush: (branch: string) => void;
}

/**
 * The branch context menu (right-click a branch row, sidebar ⋯, or toolbar
 * Branch button). A full Fork-style menu — checkout, worktree, fast-forward,
 * push, merge/rebase, new branch/tag, tracking submenu, rename, delete, AI,
 * copy — rendered through the generic {@link ContextMenu}. Items that only make
 * sense on a non-current branch are disabled on the current one.
 */
const BranchMenu: Component<BranchMenuProps> = (props) => {
  const b = () => props.state.branch;
  const cur = () => props.currentBranch ?? "current";
  const [upstream, setUpstream] = createSignal<string | null>(null);

  onMount(() => {
    branchUpstream(props.repoId, b()).then(setUpstream);
  });

  const result = (r: ActionResult | null) => props.onResult(r);

  // Remote refs become the Tracking submenu targets.
  const remoteRefs = (): string[] =>
    props.refs.filter((r) => r.kind === "Remote").map((r) => r.name);

  const trackingSubmenu = (): MenuEntry[] => {
    const items: MenuEntry[] = remoteRefs().map((rref) => ({
      label: rref,
      run: async () => result(await setTracking(props.repoId, b(), rref)),
    }));
    if (upstream()) {
      if (items.length > 0) items.push("divider");
      items.push({
        label: "Unset upstream",
        run: async () => result(await setTracking(props.repoId, b(), null)),
      });
    }
    if (items.length === 0) items.push({ label: "(no remotes)", disabled: true });
    return items;
  };

  const items = (): MenuEntry[] => {
    const list: MenuEntry[] = [
      {
        label: `Checkout ${b()}`,
        disabled: props.state.isCurrent,
        run: async () => result(await checkoutBranch(props.repoId, b())),
      },
      {
        label: "Checkout as Worktree…",
        run: () => props.onWorktree(b()),
      },
      "divider",
    ];

    // Remote derived from the upstream short name ("origin/main" → "origin").
    const remote = upstream()?.split("/")[0] ?? "origin";
    if (upstream()) {
      list.push({
        label: `Fast-Forward to '${upstream()}'`,
        disabled: props.state.isCurrent,
        run: async () => result(await fastForwardBranch(props.repoId, b(), upstream()!)),
      });
      list.push({
        // Pull integrates the upstream into the checked-out branch — only the
        // current branch can be pulled without a checkout.
        label: `Pull '${upstream()}'`,
        disabled: !props.state.isCurrent,
        run: async () => result(await pullCurrent(props.repoId)),
      });
    }
    list.push({
      label: `Push to '${remote}'…`,
      run: () => props.onPush(b()),
    });
    list.push({
      label: `Create Pull Request on '${remote}'…`,
      run: () => props.onCreatePr(b()),
    });
    list.push(
      "divider",
      {
        label: `Merge ${b()} into ${cur()}`,
        disabled: props.state.isCurrent,
        run: async () => result(await mergeIntoCurrent(props.repoId, b())),
      },
      {
        label: `Rebase ${cur()} onto ${b()}`,
        disabled: props.state.isCurrent || !props.currentBranch,
        run: async () => result(await rebaseCurrentOnto(props.repoId, cur(), b())),
      },
      "divider",
      {
        label: "New Branch Here…",
        shortcut: "⇧⌘B",
        run: () => props.onCreateBranch(b()),
      },
      {
        label: "New Tag Here…",
        shortcut: "⇧⌘G",
        run: () => {
          const target = b();
          props.onPrompt({
            title: `New tag at ${target}`,
            placeholder: "tag-name",
            submitLabel: "Create Tag",
            onSubmit: async (name) => result(await createTagAt(props.repoId, name, target)),
          });
        },
      },
      {
        label: "Tracking",
        submenu: trackingSubmenu(),
      },
      {
        label: "Rename…",
        run: () => {
          const oldName = b();
          props.onPrompt({
            title: `Rename ${oldName}`,
            placeholder: "new-branch-name",
            initial: oldName,
            submitLabel: "Rename",
            onSubmit: async (name) => result(await renameBranch(props.repoId, oldName, name)),
          });
        },
      },
      {
        label: `Delete ${b()}`,
        danger: true,
        disabled: props.state.isCurrent,
        shortcut: "⌫",
        run: async () => result(await deleteBranch(props.repoId, b())),
      },
      "divider",
      { label: "Explain changes", run: () => props.onAiExplain(b()) },
      { label: "Copy Branch Name", shortcut: "⌘C", run: async () => result(await copyBranchName(b())) },
    );
    return list;
  };

  return <ContextMenu x={props.state.x} y={props.state.y} items={items()} onClose={props.onClose} />;
};

export default BranchMenu;
