import type { Component } from "solid-js";
import type { RefInfo, RepoId } from "./commands";
import ContextMenu from "./ContextMenu";
import type { MenuEntry } from "./ContextMenu";
import { ActionResult, rebaseCurrentOnto } from "./branchActions";
import { copyRemoteLink, resetTo, revertCommit } from "./commitActions";
import { checkoutTag, copyTagName, deleteTagLocal, deleteTagOrigin } from "./tagActions";

export interface TagMenuState {
  tag: string;
  x: number;
  y: number;
}

interface TagMenuProps {
  repoId: RepoId;
  /** Name of the current (HEAD) branch; null when detached. Gates reset/rebase. */
  currentBranch: string | null;
  /** All refs, to resolve the tag's target oid. */
  refs: RefInfo[];
  state: TagMenuState;
  onClose: () => void;
  onResult: (r: ActionResult | null) => void;
  /** Open the "new branch from <startPoint>" name modal. */
  onCreateBranch: (startPoint: string) => void;
  /** Open the AI view to explain the tagged commit. */
  onAiExplain: (oid: string) => void;
  /** Open the push confirmation dialog to push this tag. */
  onPush: (tag: string) => void;
}

/**
 * The tag context menu (right-click a tag in the sidebar Tags panel). Mirrors
 * {@link BranchMenu}/CommitMenu: rebase current onto the tag, checkout (detached),
 * explain, branch/reset/revert at the tagged commit, delete locally / on origin,
 * and copy name/link. Reset and rebase are disabled on a detached HEAD.
 */
const TagMenu: Component<TagMenuProps> = (props) => {
  const tag = () => props.state.tag;
  const cur = () => props.currentBranch ?? "current";
  const canRewrite = () => props.currentBranch != null;
  const target = (): string | null =>
    props.refs.find((r) => r.kind === "Tag" && r.name === tag())?.target ?? null;

  const result = (r: ActionResult | null) => props.onResult(r);

  const resetSubmenu = (oid: string): MenuEntry[] => [
    { label: "Soft (keep changes staged)", run: async () => result(await resetTo(props.repoId, oid, "Soft")) },
    { label: "Mixed (unstage changes)", run: async () => result(await resetTo(props.repoId, oid, "Mixed")) },
    { label: "Hard (discard changes)", danger: true, run: async () => result(await resetTo(props.repoId, oid, "Hard")) },
  ];

  const items = (): MenuEntry[] => {
    const oid = target();
    const list: MenuEntry[] = [
      {
        label: `Rebase ${cur()} onto ${tag()}`,
        disabled: !canRewrite(),
        run: async () => result(await rebaseCurrentOnto(props.repoId, cur(), tag())),
      },
      { label: `Checkout Commit at ${tag()}`, run: async () => result(await checkoutTag(props.repoId, tag())) },
      "divider",
      { label: "New Branch Here…", shortcut: "⇧⌘B", run: () => props.onCreateBranch(tag()) },
      {
        label: `Reset ${cur()} to Here`,
        disabled: !canRewrite() || !oid,
        submenu: oid ? resetSubmenu(oid) : [{ label: "(unresolved tag)", disabled: true }],
      },
      { label: "Revert Commit", disabled: !oid, run: async () => { if (oid) result(await revertCommit(props.repoId, oid)); } },
      { label: "Explain Commit", disabled: !oid, run: () => { if (oid) props.onAiExplain(oid); } },
      "divider",
      { label: `Delete ${tag()} Locally`, danger: true, run: async () => result(await deleteTagLocal(props.repoId, tag())) },
      { label: `Delete ${tag()} from origin`, danger: true, run: async () => result(await deleteTagOrigin(props.repoId, "origin", tag())) },
      { label: `Push ${tag()} to origin`, run: () => props.onPush(tag()) },
      "divider",
      { label: "Copy Tag Name", shortcut: "⌘C", run: async () => result(await copyTagName(tag())) },
      { label: "Copy Link to Tag", run: async () => result(await copyRemoteLink(props.repoId, { kind: "Tag", value: tag() })) },
    ];
    return list;
  };

  return <ContextMenu x={props.state.x} y={props.state.y} items={items()} onClose={props.onClose} />;
};

export default TagMenu;
