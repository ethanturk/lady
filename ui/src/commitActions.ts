import { invoke } from "@tauri-apps/api/core";
import type { ApplyOutcome, CommitMeta, RebaseOutcome, RebaseStep, RepoId, ResetMode, WebTarget } from "./commands";
import type { ActionResult } from "./branchActions";
import { createBranchAt } from "./branchActions";

const short = (oid: string) => oid.slice(0, 8);

/** The shape `rebase_range` returns: the `onto` base + the range commits (oldest first). */
interface RebaseRange {
  onto: string;
  commits: CommitMeta[];
}

/**
 * Commit operations shared by the commit-graph context menu. Each wraps a typed
 * IPC call and returns a human-readable {@link ActionResult} (the caller owns how
 * it is shown). History-rewrite helpers (drop / reword / move) are built on the
 * existing interactive-rebase engine (`rebase_range` + `rebase_interactive`) and
 * return the raw {@link RebaseOutcome} so the caller can route conflicts into the
 * 3-pane resolver via App's `onRebaseComplete`.
 */
export async function checkoutCommit(repo: RepoId, oid: string): Promise<ActionResult> {
  try {
    await invoke("checkout", { repo, target: oid, force: false });
    return { ok: true, message: `Checked out ${short(oid)} (detached).` };
  } catch (e) {
    if (confirm(`${e}\n\nForce checkout ${short(oid)} and discard local changes?`)) {
      try {
        await invoke("checkout", { repo, target: oid, force: true });
        return { ok: true, message: `Force-checked out ${short(oid)} (detached).` };
      } catch (e2) {
        return { ok: false, message: String(e2) };
      }
    }
    return { ok: false, message: String(e) };
  }
}

/** Create a branch at `oid` (delegates to the shared branch helper). */
export async function createBranchAtCommit(repo: RepoId, name: string, oid: string): Promise<ActionResult> {
  return createBranchAt(repo, name, oid);
}

const RESET_VERB: Record<ResetMode, string> = {
  Soft: "soft",
  Mixed: "mixed",
  Hard: "hard",
};

/** `git reset --soft|--mixed|--hard <oid>` — moves the current branch to `oid`. */
export async function resetTo(repo: RepoId, oid: string, mode: ResetMode): Promise<ActionResult | null> {
  if (mode === "Hard" && !confirm(`Hard reset to ${short(oid)}? This discards working-tree changes.`)) {
    return null;
  }
  try {
    await invoke("reset", { repo, target: oid, mode });
    return { ok: true, message: `Reset (${RESET_VERB[mode]}) to ${short(oid)}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Revert `oid` (creates an inverse commit); may stop with conflicts. */
export async function revertCommit(repo: RepoId, oid: string): Promise<ActionResult | null> {
  if (!confirm(`Revert commit ${short(oid)}?`)) return null;
  try {
    const outcome = await invoke<ApplyOutcome>("revert", { repo, oid });
    if (outcome.kind === "Conflicts")
      return { ok: false, message: `Revert stopped with ${outcome.value.length} conflict(s).` };
    return { ok: true, message: `Reverted ${short(oid)} as ${short(outcome.value)}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Cherry-pick `oid` onto the current branch; may stop with conflicts. */
export async function cherryPickCommit(repo: RepoId, oid: string): Promise<ActionResult> {
  try {
    const outcome = await invoke<ApplyOutcome>("cherry_pick", { repo, oid });
    if (outcome.kind === "Conflicts")
      return { ok: false, message: `Cherry-pick stopped with ${outcome.value.length} conflict(s).` };
    return { ok: true, message: `Cherry-picked ${short(oid)} as ${short(outcome.value)}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Copy a commit's full SHA to the clipboard. */
export async function copyCommitSha(oid: string): Promise<ActionResult> {
  try {
    await navigator.clipboard.writeText(oid);
    return { ok: true, message: `Copied ${short(oid)}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/**
 * Copy a forge browser link for `target` (commit / branch / tag) to the
 * clipboard. Returns an error result when the repo has no recognized forge
 * remote (the backend returns `null`).
 */
export async function copyRemoteLink(repo: RepoId, target: WebTarget): Promise<ActionResult> {
  try {
    const url = await invoke<string | null>("remote_web_url", { repo, target });
    if (!url) return { ok: false, message: "No forge remote to build a link from." };
    await navigator.clipboard.writeText(url);
    return { ok: true, message: `Copied link: ${url}` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/**
 * Drop `oid` from history: rebase its range onto `oid`'s parent with that commit
 * marked `Drop` and its descendants `Pick`ed. Returns the rebase outcome (route
 * conflicts into the resolver).
 */
export async function dropCommit(repo: RepoId, oid: string): Promise<RebaseOutcome> {
  const range = await invoke<RebaseRange>("rebase_range", { repo, from: oid });
  const plan: RebaseStep[] = range.commits.map((c) => ({
    oid: c.oid,
    action: c.oid === oid ? "Drop" : "Pick",
    message: null,
  }));
  return invoke<RebaseOutcome>("rebase_interactive", { repo, onto: range.onto, plan });
}

/** Reword `oid`'s message via a one-step interactive rebase (`Reword`). */
export async function rewordCommit(repo: RepoId, oid: string, message: string): Promise<RebaseOutcome> {
  const range = await invoke<RebaseRange>("rebase_range", { repo, from: oid });
  const plan: RebaseStep[] = range.commits.map((c) => ({
    oid: c.oid,
    action: c.oid === oid ? "Reword" : "Pick",
    message: c.oid === oid ? message : null,
  }));
  return invoke<RebaseOutcome>("rebase_interactive", { repo, onto: range.onto, plan });
}

/**
 * Move `oid` one step *down* in the graph (toward older history) by swapping it
 * with its parent. Rebases the range starting at the parent with the two commits
 * reordered. Throws when `oid` is a root commit (no parent to swap with).
 */
export async function moveCommitDown(repo: RepoId, oid: string): Promise<RebaseOutcome> {
  // The parent of `oid` is the `onto` of its own range.
  const own = await invoke<RebaseRange>("rebase_range", { repo, from: oid });
  const parent = own.onto;
  if (!parent) throw new Error("Cannot move a root commit down.");
  // Rebase from the parent so both commits are in the range, then swap them.
  const range = await invoke<RebaseRange>("rebase_range", { repo, from: parent });
  const idx = range.commits.findIndex((c) => c.oid === oid);
  // `oid` should sit right after its parent (index 1) in the range.
  if (idx <= 0) throw new Error("Commit is already at the bottom of its history.");
  const reordered = [...range.commits];
  [reordered[idx - 1], reordered[idx]] = [reordered[idx], reordered[idx - 1]];
  const plan: RebaseStep[] = reordered.map((c) => ({ oid: c.oid, action: "Pick", message: null }));
  return invoke<RebaseOutcome>("rebase_interactive", { repo, onto: range.onto, plan });
}
