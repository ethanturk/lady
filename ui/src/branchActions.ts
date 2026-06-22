import { invoke } from "@tauri-apps/api/core";
import type { MergeOutcome, RebaseOutcome, RepoId } from "./commands";

/** Outcome of a branch action, surfaced by the caller as a notice or error. */
export interface ActionResult {
  ok: boolean;
  message: string;
}

const short = (oid: string) => oid.slice(0, 8);

/**
 * Branch operations shared by the sidebar context menu (and reusable by the
 * Refs view). Each wraps a typed IPC call, handles the confirm/force-fallback
 * dialogs git needs, and returns a human-readable result instead of touching UI
 * state — callers own how they show it. All commands already exist server-side;
 * Rename is intentionally absent (no backend command yet).
 */
export async function checkoutBranch(repo: RepoId, name: string): Promise<ActionResult> {
  try {
    await invoke("checkout", { repo, target: name, force: false });
    return { ok: true, message: `Checked out ${name}.` };
  } catch (e) {
    // git refused (e.g. local changes) — offer a discarding force checkout.
    if (confirm(`${e}\n\nForce checkout '${name}' and discard local changes?`)) {
      try {
        await invoke("checkout", { repo, target: name, force: true });
        return { ok: true, message: `Force-checked out ${name}.` };
      } catch (e2) {
        return { ok: false, message: String(e2) };
      }
    }
    return { ok: false, message: String(e) };
  }
}

/**
 * Create branch `name` at `startPoint`. The name is collected by an in-app
 * modal (NOT window.prompt — Tauri's webview does not support it), so this is a
 * plain create call.
 */
export async function createBranchAt(repo: RepoId, name: string, startPoint: string): Promise<ActionResult> {
  try {
    await invoke("create_branch", { repo, name, startPoint });
    return { ok: true, message: `Created ${name} from ${startPoint}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

export async function mergeIntoCurrent(repo: RepoId, source: string): Promise<ActionResult> {
  try {
    const outcome = await invoke<MergeOutcome>("merge", {
      repo,
      source,
      fastForward: "Auto",
      commitMessage: null,
    });
    switch (outcome.kind) {
      case "AlreadyUpToDate":
        return { ok: true, message: "Already up to date." };
      case "FastForwarded":
        return { ok: true, message: `Fast-forwarded to ${source}.` };
      case "Merged":
        return { ok: true, message: `Merged ${source} as ${short(outcome.value)}.` };
      case "Conflicts":
        return {
          ok: false,
          message: `Merge of ${source} stopped with ${outcome.value.length} conflict(s).`,
        };
    }
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

export async function rebaseCurrentOnto(
  repo: RepoId,
  currentBranch: string,
  onto: string,
): Promise<ActionResult> {
  try {
    const outcome = await invoke<RebaseOutcome>("rebase", { repo, branch: currentBranch, onto });
    if (outcome.kind === "Rebased") return { ok: true, message: `Rebased ${currentBranch} onto ${onto}.` };
    if (outcome.kind === "Stopped")
      return { ok: false, message: "Rebase stopped to edit a commit — continue or abort." };
    return {
      ok: false,
      message: `Rebase onto ${onto} stopped with ${outcome.value.length} conflict(s).`,
    };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

export async function deleteBranch(repo: RepoId, name: string): Promise<ActionResult | null> {
  if (!confirm(`Delete branch '${name}'?`)) return null;
  try {
    await invoke("delete_branch", { repo, name, force: false });
    return { ok: true, message: `Deleted ${name}.` };
  } catch (e) {
    // Unmerged branch → git refuses with -d; offer the forceful -D.
    if (confirm(`${e}\n\nForce-delete '${name}' anyway?`)) {
      try {
        await invoke("delete_branch", { repo, name, force: true });
        return { ok: true, message: `Force-deleted ${name}.` };
      } catch (e2) {
        return { ok: false, message: String(e2) };
      }
    }
    return { ok: false, message: String(e) };
  }
}

export async function copyBranchName(name: string): Promise<ActionResult> {
  try {
    await navigator.clipboard.writeText(name);
    return { ok: true, message: `Copied '${name}'.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Rename a branch (`git branch -m`). The new name comes from an in-app modal. */
export async function renameBranch(repo: RepoId, oldName: string, newName: string): Promise<ActionResult> {
  try {
    await invoke("rename_branch", { repo, old: oldName, new: newName });
    return { ok: true, message: `Renamed ${oldName} → ${newName}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** The short upstream of `branch` (e.g. `origin/main`), or null when unset. */
export async function branchUpstream(repo: RepoId, branch: string): Promise<string | null> {
  try {
    return await invoke<string | null>("branch_upstream", { repo, branch });
  } catch {
    return null;
  }
}

/** Fast-forward `branch` to its `upstream` without checking it out. */
export async function fastForwardBranch(repo: RepoId, branch: string, upstream: string): Promise<ActionResult> {
  try {
    await invoke("fast_forward_branch", { repo, branch, upstream });
    return { ok: true, message: `Fast-forwarded ${branch} to ${upstream}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Rebase `branch` onto `onto` (non-interactive). */
export async function rebaseBranchOnto(repo: RepoId, branch: string, onto: string): Promise<RebaseOutcome> {
  return invoke<RebaseOutcome>("rebase", { repo, branch, onto });
}

/** Set (`upstream`) or unset (`null`) a branch's tracking ref. */
export async function setTracking(repo: RepoId, branch: string, upstream: string | null): Promise<ActionResult> {
  try {
    await invoke("set_branch_upstream", { repo, branch, upstream });
    return {
      ok: true,
      message: upstream ? `${branch} now tracks ${upstream}.` : `${branch} tracking cleared.`,
    };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Push `branch` (setting upstream when it has none). */
export async function pushBranch(repo: RepoId, branch: string, setUpstream: boolean): Promise<ActionResult> {
  try {
    await invoke("push", { repo, remote: null, branch, setUpstream, force: false });
    return { ok: true, message: `Pushed ${branch}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Pull the current branch from its upstream (fetch + integrate). */
export async function pullCurrent(repo: RepoId): Promise<ActionResult> {
  try {
    await invoke("pull", { repo, remote: null, branch: null });
    return { ok: true, message: "Pulled." };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Create a worktree for `branch` at a user-picked directory. */
export async function addWorktreeFor(repo: RepoId, branch: string, dir: string): Promise<ActionResult> {
  try {
    await invoke("add_worktree", { repo, path: dir, branch, newBranch: false });
    return { ok: true, message: `Created worktree for ${branch} at ${dir}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Create a tag at `target` (lightweight). The name comes from an in-app modal. */
export async function createTagAt(repo: RepoId, name: string, target: string): Promise<ActionResult> {
  try {
    await invoke("create_tag", { repo, name, target, message: null });
    return { ok: true, message: `Tagged ${target} as ${name}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Create an annotated tag at `target` (carries a `message`). */
export async function createAnnotatedTagAt(repo: RepoId, name: string, target: string, message: string): Promise<ActionResult> {
  try {
    await invoke("create_tag", { repo, name, target, message });
    return { ok: true, message: `Annotated tag ${name} at ${short(target)}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}
