import type { Component } from "solid-js";
import type { RefInfo, RepoId } from "./commands";
import ContextMenu from "./ContextMenu";
import type { MenuEntry } from "./ContextMenu";
import { ActionResult, rebaseBranchOnto } from "./branchActions";
import { copyRemoteLink, resetTo, revertCommit } from "./commitActions";
import { checkoutTag, copyTagName, deleteTagLocal, deleteTagOrigin, mergeIntoTag, moveTag } from "./tagActions";

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
  /** Name of the default branch (e.g. main), used for tag-into-branch ops. */
  defaultBranch: string;
  state: TagMenuState;
  onClose: () => void;
  onResult: (r: ActionResult | null) => void;
  /** Called when the operation mutates refs so the parent can refresh immediately. */
  onMutate: () => void;
  /** Open the "new branch from <startPoint>" name modal. */
  onCreateBranch: (startPoint: string) => void;
  /** Open the AI view to explain the tagged commit. */
  onAiExplain: (oid: string) => void;
  /** Open the push confirmation dialog to push this tag. */
  onPush: (tag: string) => void;
  /** Open the interactive-rebase editor for `branch` onto `tag`. */
  onInteractiveRebase: (branch: string, ontoTag: string) => void;
}

/**
 * The tag context menu (right-click a tag in the sidebar Tags panel). Adds tag-
 * specific operations against the default branch: fast-forward the tag, merge
 * the default branch into the tag, rebase the default branch onto the tag, and
 * interactive-rebase the default branch onto the tag. Also includes checkout,
 * new branch, reset/revert/explain, delete, push, and copy actions.
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
    const base = props.defaultBranch;
    const list: MenuEntry[] = [
      {
        label: `Fast forward ${tag()} to ${base}`,
        run: async () => result(await moveTag(props.repoId, tag(), base, props.onMutate)),
      },
      {
        label: `Merge ${base} into ${tag()}`,
        run: async () => result(await mergeIntoTag(props.repoId, base, props.onMutate)),
      },
      {
        label: `Rebase ${base} into ${tag()}`,
        run: async () => {
          try {
            const outcome = await rebaseBranchOnto(props.repoId, base, tag());
            props.onMutate();
            if (outcome.kind === "Rebased") {
              result({ ok: true, message: `Rebased ${base} onto ${tag()}.` });
            } else if (outcome.kind === "Stopped") {
              result({ ok: false, message: "Rebase stopped to edit a commit — continue or abort." });
            } else {
              result({ ok: false, message: `Rebase stopped with ${outcome.value.length} conflict(s).` });
            }
          } catch (e) {
            result({ ok: false, message: String(e) });
          }
        },
      },
      {
        label: `Interactive Rebase ${base} into ${tag()}`,
        run: () => props.onInteractiveRebase(base, tag()),
      },
      "divider",
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
      { label: `Delete ${tag()} Locally`, danger: true, run: async () => result(await deleteTagLocal(props.repoId, tag(), props.onMutate)) },
      { label: `Delete ${tag()} from origin`, danger: true, run: async () => result(await deleteTagOrigin(props.repoId, "origin", tag(), props.onMutate)) },
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
