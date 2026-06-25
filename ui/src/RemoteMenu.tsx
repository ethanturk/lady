import type { Component } from "solid-js";
import type { RepoId } from "./commands";
import ContextMenu from "./ContextMenu";
import type { MenuEntry } from "./ContextMenu";
import { PromptSpec } from "./BranchMenu";
import {
  ActionResult,
  checkoutBranch,
  copyBranchName,
  createTagAt,
  fetchRemote,
  mergeIntoCurrent,
  pullRemote,
  rebaseCurrentOnto,
} from "./branchActions";
import { copyRemoteLink } from "./commitActions";
import { deleteRemoteBranch } from "./tagActions";

export interface RemoteMenuState {
  /** Short remote-tracking ref name, e.g. `origin/main`. */
  remoteRef: string;
  x: number;
  y: number;
}

interface RemoteMenuProps {
  repoId: RepoId;
  /** Name of the current (HEAD) branch, for the merge/rebase labels. */
  currentBranch: string | null;
  state: RemoteMenuState;
  onClose: () => void;
  /** Result (or null if the action was cancelled) — caller shows + refreshes. */
  onResult: (r: ActionResult | null) => void;
  /** Fired after an op that mutates refs so the parent can refresh immediately. */
  onMutate: () => void;
  /** Open the "new branch from <startPoint>" name modal (avoids window.prompt). */
  onCreateBranch: (startPoint: string) => void;
  /** Open a generalized name prompt (New Tag). */
  onPrompt: (spec: PromptSpec) => void;
  /** Pick a directory then create a worktree for the branch there. */
  onWorktree: (branch: string) => void;
  /** Open the AI view to explain this branch. */
  onAiExplain: (branch: string) => void;
}

/**
 * The remote-tracking branch context menu (right-click a row in the sidebar
 * Remote panel, or the row ⋯ button). A near-mirror of {@link BranchMenu}, with
 * the operations that make sense for a remote-tracking ref like `origin/main`:
 * checkout (DWIM-creates the local tracking branch), worktree, fetch/pull,
 * merge/rebase into the current branch, new branch/tag here, delete on the
 * remote, AI explain, and copy. Operations specific to local branches (push,
 * rename, tracking) are intentionally omitted.
 */
const RemoteMenu: Component<RemoteMenuProps> = (props) => {
  const ref = () => props.state.remoteRef;
  const cur = () => props.currentBranch ?? "current";
  // `origin/main` → remote `origin`, branch `main` (branch parts may nest).
  const remote = () => ref().split("/")[0];
  const branchPart = () => ref().split("/").slice(1).join("/");

  const result = (r: ActionResult | null) => props.onResult(r);

  const items = (): MenuEntry[] => {
    const b = branchPart();
    return [
      {
        // git DWIMs `checkout <branch>` to create a local branch tracking the
        // same-named remote ref.
        label: `Checkout ${b}`,
        run: async () => result(await checkoutBranch(props.repoId, b)),
      },
      {
        label: "Checkout as Worktree…",
        run: () => props.onWorktree(b),
      },
      "divider",
      {
        label: `Fetch '${remote()}'`,
        run: async () => {
          const r = await fetchRemote(props.repoId, remote());
          props.onMutate();
          result(r);
        },
      },
      {
        // Pull integrates the remote branch into the checked-out branch.
        label: `Pull '${ref()}' into ${cur()}`,
        disabled: !props.currentBranch,
        run: async () => {
          const r = await pullRemote(props.repoId, remote(), b);
          props.onMutate();
          result(r);
        },
      },
      "divider",
      {
        label: `Merge ${ref()} into ${cur()}`,
        disabled: !props.currentBranch,
        run: async () => result(await mergeIntoCurrent(props.repoId, ref())),
      },
      {
        label: `Rebase ${cur()} onto ${ref()}`,
        disabled: !props.currentBranch,
        run: async () => result(await rebaseCurrentOnto(props.repoId, cur(), ref())),
      },
      "divider",
      {
        label: "New Branch Here…",
        shortcut: "⇧⌘B",
        run: () => props.onCreateBranch(ref()),
      },
      {
        label: "New Tag Here…",
        shortcut: "⇧⌘G",
        run: () => {
          const target = ref();
          props.onPrompt({
            title: `New tag at ${target}`,
            placeholder: "tag-name",
            submitLabel: "Create Tag",
            onSubmit: async (name) => result(await createTagAt(props.repoId, name, target)),
          });
        },
      },
      "divider",
      {
        label: `Delete ${b} from '${remote()}'`,
        danger: true,
        run: async () => {
          const r = await deleteRemoteBranch(props.repoId, remote(), b);
          if (r) props.onMutate();
          result(r);
        },
      },
      "divider",
      { label: "Explain changes", run: () => props.onAiExplain(ref()) },
      { label: "Copy Branch Name", shortcut: "⌘C", run: async () => result(await copyBranchName(ref())) },
      { label: "Copy Link to Branch", run: async () => result(await copyRemoteLink(props.repoId, { kind: "Branch", value: b })) },
    ];
  };

  return <ContextMenu x={props.state.x} y={props.state.y} items={items()} onClose={props.onClose} />;
};

export default RemoteMenu;
