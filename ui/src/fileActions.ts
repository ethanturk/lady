import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import type { RepoId } from "./commands";
import type { ActionResult } from "./branchActions";

/**
 * File / folder context-menu actions for the Local Changes view. Each wraps a
 * typed IPC call (or a clipboard / dialog interaction) and returns a
 * human-readable {@link ActionResult}; callers own how they surface it. Folder
 * variants pass every path under the folder so one action covers the subtree.
 */

/** Open a repo-relative file with the OS default application. */
export async function openFile(repo: RepoId, path: string): Promise<ActionResult> {
  try {
    await invoke("open_path", { repo, path });
    return { ok: true, message: `Opened ${path}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Reveal a repo-relative file in the OS file manager. */
export async function revealFile(repo: RepoId, path: string): Promise<ActionResult> {
  try {
    await invoke("reveal_path", { repo, path });
    return { ok: true, message: `Revealed ${path}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Discard all working-tree + index changes to `paths` (after a confirm). */
export async function discardChanges(repo: RepoId, paths: string[]): Promise<ActionResult | null> {
  const noun = paths.length === 1 ? paths[0] : `${paths.length} files`;
  if (!confirm(`Discard changes to ${noun}? This cannot be undone.`)) return null;
  try {
    await invoke("discard_files", { repo, paths });
    return { ok: true, message: `Discarded changes to ${noun}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Stash only `paths` (leaving the rest of the working tree intact). */
export async function stashFiles(repo: RepoId, paths: string[]): Promise<ActionResult> {
  const noun = paths.length === 1 ? paths[0] : `${paths.length} files`;
  try {
    await invoke("stash_paths", { repo, message: null, includeUntracked: true, paths });
    return { ok: true, message: `Stashed ${noun}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Export the uncommitted diff of `paths` to a file chosen via the save dialog. */
export async function saveAsPatch(repo: RepoId, paths: string[]): Promise<ActionResult | null> {
  let dest: string | null;
  try {
    dest = await saveDialog({
      title: "Save patch",
      defaultPath: "changes.patch",
      filters: [{ name: "Patch", extensions: ["patch", "diff"] }],
    });
  } catch (e) {
    return { ok: false, message: String(e) };
  }
  if (!dest) return null;
  try {
    await invoke("export_patch", { repo, paths, dest });
    return { ok: true, message: `Saved patch to ${dest}.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Append ignore `patterns` to the repo-root .gitignore. */
export async function ignore(repo: RepoId, patterns: string[]): Promise<ActionResult> {
  try {
    await invoke("add_to_gitignore", { repo, patterns });
    return { ok: true, message: `Added ${patterns.length} pattern(s) to .gitignore.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

/** Copy a path (file or folder) to the clipboard. */
export async function copyPath(path: string): Promise<ActionResult> {
  try {
    await navigator.clipboard.writeText(path);
    return { ok: true, message: `Copied '${path}'.` };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}
