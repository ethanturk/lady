import { invoke } from "@tauri-apps/api/core";
import type { MergeOutcome, RepoId } from "./commands";
import type { ActionResult } from "./branchActions";

/**
 * Tag operations shared by the tag context menu (sidebar Tags panel). Each wraps
 * a typed IPC call and returns a human-readable {@link ActionResult}; the caller
 * owns how it is shown. `copyRemoteLink` for a tag lives in `commitActions` (it
 * is target-generic).
 *
 * The optional `onMutate` callback is fired after any operation that changes
 * refs, so callers can refresh refs/graph immediately.
 */

/** Checkout the commit a tag points at (detached HEAD). */
export async function checkoutTag(repo: RepoId, tag: string): Promise<ActionResult> {
  try {
    await invoke("checkout", { repo, target: tag, force: false });
    return { ok: true, message: `Checked out ${tag} (detached).` };
  } catch (e) {
    if (confirm(`${e}\n\nForce checkout '${tag}' and discard local changes?`)) {
      try {
        await invoke("checkout", { repo, target: tag, force: true });
        return { ok: true, message: `Force-checked out ${tag} (detached).` };
      } catch (e2) {
        return { ok: false, message: String(e2) };
      }
    }
    return { ok: false, message: String(e) };
  }
}

/** Move (fast-forward) an existing tag to `target`. */
export async function moveTag(
  repo: RepoId,
  tag: string,
  target: string,
  onMutate?: () => void,
): Promise<ActionResult> {
  try {
    await invoke("move_tag", { repo, name: tag, target });
    onMutate?.();
    return { ok: true, message: `Moved ${tag} to ${target}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Merge `source` into the checked-out tag (detached HEAD). */
export async function mergeIntoTag(
  repo: RepoId,
  source: string,
  onMutate?: () => void,
): Promise<ActionResult> {
  try {
    await invoke("checkout", { repo, target: "HEAD", force: false });
    const outcome = await invoke<MergeOutcome>("merge", {
      repo,
      source,
      fastForward: "Auto",
      commitMessage: null,
    });
    onMutate?.();
    switch (outcome.kind) {
      case "AlreadyUpToDate":
        return { ok: true, message: "Already up to date." };
      case "FastForwarded":
        return { ok: true, message: `Fast-forwarded to ${source}.` };
      case "Merged":
        return { ok: true, message: `Merged ${source} as ${outcome.value.slice(0, 8)}.` };
      case "Conflicts":
        return { ok: false, message: `Merge stopped with ${outcome.value.length} conflict(s).` };
    }
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Delete a tag locally (`git tag -d`). */
export async function deleteTagLocal(
  repo: RepoId,
  tag: string,
  onMutate?: () => void,
): Promise<ActionResult | null> {
  if (!confirm(`Delete tag '${tag}' locally?`)) return null;
  try {
    await invoke("delete_tag", { repo, name: tag });
    onMutate?.();
    return { ok: true, message: `Deleted tag ${tag}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Delete a tag on `remote` (`git push <remote> :refs/tags/<tag>`). */
export async function deleteTagOrigin(
  repo: RepoId,
  remote: string,
  tag: string,
  onMutate?: () => void,
): Promise<ActionResult | null> {
  if (!confirm(`Delete tag '${tag}' from '${remote}'?`)) return null;
  try {
    await invoke("delete_remote_ref", { repo, remote, refspec: `refs/tags/${tag}` });
    onMutate?.();
    return { ok: true, message: `Deleted tag ${tag} from ${remote}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Copy a tag's name to the clipboard. */
export async function copyTagName(tag: string): Promise<ActionResult> {
  try {
    await navigator.clipboard.writeText(tag);
    return { ok: true, message: `Copied '${tag}'.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Delete a branch on `remote` (`git push <remote> :refs/heads/<branch>`). */
export async function deleteRemoteBranch(repo: RepoId, remote: string, branch: string): Promise<ActionResult | null> {
  if (!confirm(`Delete branch '${branch}' from '${remote}'?`)) return null;
  try {
    await invoke("delete_remote_ref", { repo, remote, refspec: `refs/heads/${branch}` });
    return { ok: true, message: `Deleted ${branch} from ${remote}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}
