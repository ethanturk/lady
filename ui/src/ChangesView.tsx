import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ChangeKind, DiffSpec, FileStatus, RepoId, WorkingTree } from "./commands";
import DiffView from "./DiffView";

/** A short colored badge for a file's change kind. */
const KIND_BADGE: Record<ChangeKind, { label: string; color: string }> = {
  Added: { label: "A", color: "#1a7f37" },
  Modified: { label: "M", color: "#9a6700" },
  Deleted: { label: "D", color: "#cf222e" },
  Renamed: { label: "R", color: "#0969da" },
  Untracked: { label: "?", color: "#6e7781" },
  Conflicted: { label: "!", color: "#cf222e" },
};

const Badge: Component<{ kind: ChangeKind }> = (props) => {
  const b = () => KIND_BADGE[props.kind];
  return (
    <span
      style={{
        display: "inline-block",
        width: "1.1rem",
        "text-align": "center",
        "font-family": "monospace",
        "font-weight": 700,
        "font-size": "0.75rem",
        color: b().color,
      }}
      title={props.kind}
    >
      {b().label}
    </span>
  );
};

const smallBtn = {
  border: "1px solid #ccc",
  background: "#fff",
  "border-radius": "3px",
  "font-size": "0.7rem",
  padding: "0 0.4rem",
  cursor: "pointer",
};

/** One row in a changes bucket: badge + path (rename shows old → new) + action. */
const Row: Component<{
  file: FileStatus;
  actionLabel: string;
  onAction: () => void;
  selected: boolean;
  onSelect: () => void;
}> = (props) => (
  <li
    style={{
      display: "flex",
      "align-items": "center",
      gap: "0.4rem",
      padding: "0.1rem 0.25rem",
      "font-family": "monospace",
      "font-size": "0.8rem",
      background: props.selected ? "#dbeafe" : "transparent",
      "border-radius": "3px",
    }}
  >
    <Badge kind={props.file.kind} />
    <span
      onClick={props.onSelect}
      title="Show diff"
      style={{
        flex: "1",
        cursor: "pointer",
        overflow: "hidden",
        "text-overflow": "ellipsis",
        "white-space": "nowrap",
      }}
    >
      <Show when={props.file.old_path}>
        <span style={{ color: "#888" }}>{props.file.old_path} → </span>
      </Show>
      {props.file.path}
    </span>
    <button style={smallBtn} onClick={props.onAction}>
      {props.actionLabel}
    </button>
  </li>
);

/** Which file + side the diff pane is showing. */
interface Selection {
  path: string;
  staged: boolean;
}

interface ChangesViewProps {
  repoId: RepoId;
  /** Bump to force a status reload after an external mutation. */
  refreshNonce?: number;
  /** Called after a mutation here so sibling views (refs/graph) can reload. */
  onChanged?: () => void;
}

/**
 * The Changes view: the working-tree surface for staging and committing.
 * Renders staged / unstaged / untracked buckets with per-file and bulk
 * stage/unstage actions; later stories add diffs and commit on top.
 */
const ChangesView: Component<ChangesViewProps> = (props) => {
  const [wt, setWt] = createSignal<WorkingTree | null>(null);
  const [err, setErr] = createSignal<string | null>(null);
  const [selected, setSelected] = createSignal<Selection | null>(null);

  const reload = () => {
    setErr(null);
    invoke<WorkingTree>("status", { repo: props.repoId })
      .then(setWt)
      .catch((e) => setErr(String(e)));
  };

  // The DiffSpec for the current selection (staged → index-vs-HEAD).
  const selectedSpec = (): DiffSpec | null => {
    const s = selected();
    if (!s) return null;
    return { kind: s.staged ? "IndexVsHead" : "WorkingVsIndex", value: s.path };
  };
  const isSelected = (path: string, staged: boolean) =>
    selected()?.path === path && selected()?.staged === staged;

  // Reload on repo change and whenever the refresh nonce bumps.
  createEffect(() => {
    void props.repoId;
    void props.refreshNonce;
    reload();
  });

  // After a mutation: reload local status and notify siblings (refs/graph).
  const afterMutation = () => {
    reload();
    props.onChanged?.();
  };

  const stage = (paths: string[]) => {
    if (paths.length === 0) return;
    invoke("stage_paths", { repo: props.repoId, paths })
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };
  const unstage = (paths: string[]) => {
    if (paths.length === 0) return;
    invoke("unstage_paths", { repo: props.repoId, paths })
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };

  // Stage / unstage a single hunk of a file by index (PH2-004).
  const stageHunk = (path: string, hunk: number) => {
    invoke("stage_hunks", { repo: props.repoId, path, hunks: [hunk] })
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };
  const unstageHunk = (path: string, hunk: number) => {
    invoke("unstage_hunks", { repo: props.repoId, path, hunks: [hunk] })
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };

  const untrackedAsFiles = (): FileStatus[] =>
    (wt()?.untracked ?? []).map((path) => ({ path, old_path: null, kind: "Untracked" as const }));

  // All paths in the unstaged + untracked sets (everything stageable).
  const allUnstagedPaths = (): string[] => [
    ...(wt()?.unstaged ?? []).map((f) => f.path),
    ...(wt()?.untracked ?? []),
  ];
  const allStagedPaths = (): string[] => (wt()?.staged ?? []).map((f) => f.path);

  const isClean = () =>
    wt() !== null &&
    wt()!.staged.length === 0 &&
    wt()!.unstaged.length === 0 &&
    wt()!.untracked.length === 0;

  const header = (title: string, count: number, action?: { label: string; run: () => void }) => (
    <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", margin: "0.5rem 0 0.25rem" }}>
      <h3 style={{ margin: 0, "font-size": "0.85rem" }}>
        {title} ({count})
      </h3>
      <Show when={action}>
        <button style={smallBtn} onClick={action!.run}>
          {action!.label}
        </button>
      </Show>
    </div>
  );

  return (
    <div style={{ height: "100%", display: "flex", overflow: "hidden" }}>
      {/* Left: the staged / unstaged / untracked lists. */}
      <div style={{ flex: "1", "min-width": "0", "overflow-y": "auto", padding: "0.5rem 1rem" }}>
        <Show when={err()}>
          <p style={{ color: "crimson", "font-size": "0.85rem" }}>{err()}</p>
        </Show>
        <Show when={isClean()}>
          <p style={{ color: "#888", "font-size": "0.85rem" }}>Working tree clean.</p>
        </Show>

        <Show when={(wt()?.staged.length ?? 0) > 0}>
          {header("Staged", wt()!.staged.length, {
            label: "Unstage all",
            run: () => unstage(allStagedPaths()),
          })}
          <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
            <For each={wt()!.staged}>
              {(f) => (
                <Row
                  file={f}
                  actionLabel="Unstage"
                  onAction={() => unstage([f.path])}
                  selected={isSelected(f.path, true)}
                  onSelect={() => setSelected({ path: f.path, staged: true })}
                />
              )}
            </For>
          </ul>
        </Show>

        <Show when={(wt()?.unstaged.length ?? 0) > 0}>
          {header("Unstaged", wt()!.unstaged.length, {
            label: "Stage all",
            run: () => stage(allUnstagedPaths()),
          })}
          <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
            <For each={wt()!.unstaged}>
              {(f) => (
                <Row
                  file={f}
                  actionLabel="Stage"
                  onAction={() => stage([f.path])}
                  selected={isSelected(f.path, false)}
                  onSelect={() => setSelected({ path: f.path, staged: false })}
                />
              )}
            </For>
          </ul>
        </Show>

        <Show when={(wt()?.untracked.length ?? 0) > 0}>
          {header("Untracked", wt()!.untracked.length)}
          <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
            <For each={untrackedAsFiles()}>
              {(f) => (
                <Row
                  file={f}
                  actionLabel="Stage"
                  onAction={() => stage([f.path])}
                  selected={isSelected(f.path, false)}
                  onSelect={() => setSelected({ path: f.path, staged: false })}
                />
              )}
            </For>
          </ul>
        </Show>
      </div>

      {/* Right: the diff for the selected file. */}
      <Show when={selectedSpec()}>
        <div style={{ flex: "1", "min-width": "0", "border-left": "1px solid #ddd", overflow: "hidden" }}>
          <DiffView
            repoId={props.repoId}
            spec={selectedSpec()!}
            hunkActionLabel={selected()!.staged ? "Unstage hunk" : "Stage hunk"}
            onHunkAction={selected()!.staged ? unstageHunk : stageHunk}
          />
        </div>
      </Show>
    </div>
  );
};

export default ChangesView;
