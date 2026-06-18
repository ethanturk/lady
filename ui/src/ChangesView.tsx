import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ChangeKind, FileStatus, RepoId, WorkingTree } from "./commands";

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

/** One row in a changes bucket: badge + path (rename shows old → new). */
const Row: Component<{ file: FileStatus }> = (props) => (
  <li
    style={{
      display: "flex",
      "align-items": "center",
      gap: "0.4rem",
      padding: "0.1rem 0",
      "font-family": "monospace",
      "font-size": "0.8rem",
    }}
  >
    <Badge kind={props.file.kind} />
    <span style={{ overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
      <Show when={props.file.old_path}>
        <span style={{ color: "#888" }}>{props.file.old_path} → </span>
      </Show>
      {props.file.path}
    </span>
  </li>
);

interface ChangesViewProps {
  repoId: RepoId;
  /** Bump to force a status reload after an external mutation. */
  refreshNonce?: number;
  /** Called after a mutation here so sibling views (refs/graph) can reload. */
  onChanged?: () => void;
}

/**
 * The Changes view: the working-tree surface for staging and committing.
 * PH2-001 renders staged / unstaged / untracked buckets; later stories add
 * stage/unstage, diffs, and commit on top.
 */
const ChangesView: Component<ChangesViewProps> = (props) => {
  const [wt, setWt] = createSignal<WorkingTree | null>(null);
  const [err, setErr] = createSignal<string | null>(null);

  const reload = () => {
    setErr(null);
    invoke<WorkingTree>("status", { repo: props.repoId })
      .then(setWt)
      .catch((e) => setErr(String(e)));
  };

  // Reload on repo change and whenever the refresh nonce bumps.
  createEffect(() => {
    void props.repoId;
    void props.refreshNonce;
    reload();
  });

  const untrackedAsFiles = (): FileStatus[] =>
    (wt()?.untracked ?? []).map((path) => ({ path, old_path: null, kind: "Untracked" as const }));

  const isClean = () =>
    wt() !== null &&
    wt()!.staged.length === 0 &&
    wt()!.unstaged.length === 0 &&
    wt()!.untracked.length === 0;

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.5rem 1rem" }}>
      <Show when={err()}>
        <p style={{ color: "crimson", "font-size": "0.85rem" }}>{err()}</p>
      </Show>
      <Show when={isClean()}>
        <p style={{ color: "#888", "font-size": "0.85rem" }}>Working tree clean.</p>
      </Show>

      <Show when={(wt()?.staged.length ?? 0) > 0}>
        <h3 style={{ margin: "0.5rem 0 0.25rem", "font-size": "0.85rem" }}>
          Staged ({wt()!.staged.length})
        </h3>
        <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
          <For each={wt()!.staged}>{(f) => <Row file={f} />}</For>
        </ul>
      </Show>

      <Show when={(wt()?.unstaged.length ?? 0) > 0}>
        <h3 style={{ margin: "0.5rem 0 0.25rem", "font-size": "0.85rem" }}>
          Unstaged ({wt()!.unstaged.length})
        </h3>
        <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
          <For each={wt()!.unstaged}>{(f) => <Row file={f} />}</For>
        </ul>
      </Show>

      <Show when={(wt()?.untracked.length ?? 0) > 0}>
        <h3 style={{ margin: "0.5rem 0 0.25rem", "font-size": "0.85rem" }}>
          Untracked ({wt()!.untracked.length})
        </h3>
        <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
          <For each={untrackedAsFiles()}>{(f) => <Row file={f} />}</For>
        </ul>
      </Show>
    </div>
  );
};

export default ChangesView;
