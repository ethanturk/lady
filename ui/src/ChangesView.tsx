import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ChangeKind, DiffSpec, FileStatus, RepoId, StashEntry, WorkingTree } from "./commands";
import DiffView from "./DiffView";
import { cancelAi, isConsentError, runAiStream } from "./ai";

/** A short colored badge for a file's change kind. */
const KIND_BADGE: Record<ChangeKind, { label: string; color: string }> = {
  Added: { label: "A", color: "var(--success)" },
  Modified: { label: "M", color: "var(--warning)" },
  Deleted: { label: "D", color: "var(--danger)" },
  Renamed: { label: "R", color: "var(--info)" },
  Untracked: { label: "?", color: "var(--fg-muted)" },
  Conflicted: { label: "!", color: "var(--danger)" },
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
  border: "1px solid var(--border)",
  background: "var(--surface)",
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
      background: props.selected ? "var(--selection)" : "transparent",
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
        <span style={{ color: "var(--fg-muted)" }}>{props.file.old_path} → </span>
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
  const [message, setMessage] = createSignal("");
  const [amend, setAmend] = createSignal(false);
  const [sign, setSign] = createSignal(false);
  const [recent, setRecent] = createSignal<string[]>([]);
  const [stashes, setStashes] = createSignal<StashEntry[]>([]);
  const [stashUntracked, setStashUntracked] = createSignal(false);
  // AI commit-message generation (PH5-006).
  const [aiBusy, setAiBusy] = createSignal(false);
  const [aiReq, setAiReq] = createSignal<string | null>(null);

  const generateMessage = async () => {
    if (aiBusy()) return;
    setErr(null);
    setAiBusy(true);
    setMessage("");
    try {
      const full = await runAiStream(
        "ai_commit_message",
        { repo: props.repoId },
        (acc) => setMessage(acc),
        (id) => setAiReq(id),
      );
      setMessage(full);
    } catch (e) {
      const msg = String(e);
      setErr(
        isConsentError(msg)
          ? "AI consent required — enable the provider and grant consent in Settings."
          : msg,
      );
    } finally {
      setAiBusy(false);
      setAiReq(null);
    }
  };

  const cancelGenerate = () => {
    const id = aiReq();
    if (id) cancelAi(id).catch(() => {});
  };

  const reload = () => {
    setErr(null);
    invoke<WorkingTree>("status", { repo: props.repoId })
      .then(setWt)
      .catch((e) => setErr(String(e)));
    invoke<string[]>("recent_messages", { repo: props.repoId, limit: 10 })
      .then(setRecent)
      .catch(() => setRecent([]));
    invoke<StashEntry[]>("stash_list", { repo: props.repoId })
      .then(setStashes)
      .catch(() => setStashes([]));
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

  // Line-level staging and destructive discards on the unstaged side (PH2-005).
  const stageLines = (path: string, hunk: number, lines: number[]) => {
    invoke("stage_lines", { repo: props.repoId, path, hunk, lines })
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };
  const discardLines = (path: string, hunk: number, lines: number[]) => {
    invoke("discard_lines", { repo: props.repoId, path, hunk, lines })
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };
  const discardHunk = (path: string, hunk: number) => {
    invoke("discard_hunks", { repo: props.repoId, path, hunks: [hunk] })
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };

  // Toggle amend; turning it on prefills the last message if the box is empty.
  const toggleAmend = () => {
    const on = !amend();
    setAmend(on);
    if (on && message().trim() === "" && recent().length > 0) {
      setMessage(recent()[0]);
    }
  };

  // Commit is allowed when there's a message and either something is staged or
  // we're amending the tip (which can rewrite just the message).
  const canCommit = () =>
    message().trim().length > 0 && ((wt()?.staged.length ?? 0) > 0 || amend());

  const doCommit = () => {
    if (!canCommit()) return;
    invoke<string>("commit", { repo: props.repoId, message: message(), amend: amend(), sign: sign() })
      .then(() => {
        setMessage("");
        setAmend(false);
        afterMutation();
      })
      .catch((e) => setErr(String(e)));
  };

  // Stash the working tree, then refresh status + stash list + siblings.
  const [stashMsg, setStashMsg] = createSignal("");
  const stashSave = () => {
    const msg = stashMsg().trim();
    invoke("stash_save", {
      repo: props.repoId,
      message: msg === "" ? null : msg,
      includeUntracked: stashUntracked(),
    })
      .then(() => setStashMsg(""))
      .then(afterMutation)
      .catch((e) => setErr(String(e)));
  };

  // Generate a short stash note for the working changes (PH5-010).
  const generateStashNote = async () => {
    if (aiBusy()) return;
    setErr(null);
    setAiBusy(true);
    setStashMsg("");
    try {
      const full = await runAiStream(
        "ai_stash_note",
        { repo: props.repoId },
        (acc) => setStashMsg(acc),
        (id) => setAiReq(id),
      );
      setStashMsg(full.trim());
    } catch (e) {
      const msg = String(e);
      setErr(
        isConsentError(msg)
          ? "AI consent required — enable the provider and grant consent in Settings."
          : msg,
      );
    } finally {
      setAiBusy(false);
      setAiReq(null);
    }
  };
  const stashOp = (cmd: string, index: number) => {
    invoke(cmd, { repo: props.repoId, index })
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

  const commitBox = () => (
    <div
      style={{
        "border-top": "1px solid var(--border)",
        padding: "0.5rem 1rem",
        background: "var(--surface-2)",
        display: "flex",
        "flex-direction": "column",
        gap: "0.4rem",
      }}
    >
      <textarea
        value={message()}
        onInput={(e) => setMessage(e.currentTarget.value)}
        placeholder={amend() ? "Amend commit message…" : "Commit message…"}
        rows={3}
        style={{
          width: "100%",
          "box-sizing": "border-box",
          resize: "vertical",
          "font-family": "inherit",
          "font-size": "0.85rem",
          padding: "0.35rem",
          border: "1px solid var(--border)",
          "border-radius": "4px",
        }}
      />
      <div style={{ display: "flex", "align-items": "center", gap: "0.6rem", "flex-wrap": "wrap" }}>
        <button
          style={{
            border: "1px solid var(--success)",
            background: canCommit() ? "var(--success)" : "var(--success-border)",
            color: "var(--on-accent)",
            "border-radius": "4px",
            padding: "0.25rem 0.8rem",
            "font-size": "0.8rem",
            cursor: canCommit() ? "pointer" : "not-allowed",
          }}
          disabled={!canCommit()}
          onClick={doCommit}
        >
          {amend() ? "Amend" : "Commit"}
        </button>
        <label style={{ display: "flex", "align-items": "center", gap: "0.25rem", "font-size": "0.8rem" }}>
          <input type="checkbox" checked={amend()} onChange={toggleAmend} />
          Amend last commit
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.25rem", "font-size": "0.8rem" }}>
          <input type="checkbox" checked={sign()} onChange={() => setSign((s) => !s)} />
          Sign (-S)
        </label>
        <button
          style={{ "font-size": "0.78rem", padding: "0.2rem 0.6rem", border: "1px solid var(--accent)", "border-radius": "4px", background: "var(--surface)", color: "var(--accent)", cursor: aiBusy() ? "default" : "pointer" }}
          disabled={aiBusy()}
          onClick={generateMessage}
          title="Generate a commit message for the staged changes (AI)"
        >
          {aiBusy() ? "Generating…" : "✨ Generate"}
        </button>
        <Show when={aiBusy()}>
          <button style={{ "font-size": "0.78rem", padding: "0.2rem 0.6rem" }} onClick={cancelGenerate}>
            Cancel
          </button>
        </Show>
        <Show when={recent().length > 0}>
          <select
            onChange={(e) => {
              if (e.currentTarget.value) setMessage(e.currentTarget.value);
              e.currentTarget.selectedIndex = 0;
            }}
            style={{ "font-size": "0.75rem", "max-width": "12rem" }}
            title="Reuse a recent message"
          >
            <option value="">Recent…</option>
            <For each={recent()}>{(m) => <option value={m}>{m}</option>}</For>
          </select>
        </Show>
      </div>
    </div>
  );

  return (
    <div style={{ height: "100%", display: "flex", overflow: "hidden" }}>
      {/* Left: changes lists (scroll) above a pinned commit box. */}
      <div
        style={{
          flex: "1",
          "min-width": "0",
          display: "flex",
          "flex-direction": "column",
          overflow: "hidden",
        }}
      >
      <div style={{ flex: "1", "min-width": "0", "overflow-y": "auto", padding: "0.5rem 1rem" }}>
        <Show when={err()}>
          <p style={{ color: "var(--error)", "font-size": "0.85rem" }}>{err()}</p>
        </Show>
        <Show when={isClean()}>
          <p style={{ color: "var(--fg-muted)", "font-size": "0.85rem" }}>Working tree clean.</p>
        </Show>

        {/* Stash controls: save the working tree, manage the stack. */}
        <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", margin: "0.25rem 0", "flex-wrap": "wrap" }}>
          <button style={smallBtn} disabled={isClean()} onClick={stashSave}>
            Stash changes
          </button>
          <input
            style={{ flex: "1", "min-width": "8rem", "font-size": "0.75rem", padding: "0.15rem 0.35rem", border: "1px solid var(--border)", "border-radius": "4px", background: "var(--surface)", color: "var(--fg)" }}
            placeholder="stash note (optional)…"
            value={stashMsg()}
            onInput={(e) => setStashMsg(e.currentTarget.value)}
          />
          <button style={smallBtn} disabled={isClean() || aiBusy()} title="Generate a stash note with AI" onClick={generateStashNote}>
            {aiBusy() ? "…" : "✨"}
          </button>
          <label style={{ display: "flex", "align-items": "center", gap: "0.2rem", "font-size": "0.72rem", color: "var(--fg-muted)" }}>
            <input
              type="checkbox"
              checked={stashUntracked()}
              onChange={(e) => setStashUntracked(e.currentTarget.checked)}
            />
            include untracked
          </label>
        </div>

        <Show when={stashes().length > 0}>
          {header("Stashes", stashes().length)}
          <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
            <For each={stashes()}>
              {(s) => (
                <li
                  style={{
                    display: "flex",
                    "align-items": "center",
                    gap: "0.4rem",
                    padding: "0.1rem 0.25rem",
                    "font-family": "monospace",
                    "font-size": "0.78rem",
                  }}
                >
                  <span style={{ color: "var(--accent-2)" }}>{`stash@{${s.index}}`}</span>
                  <span
                    style={{
                      flex: "1",
                      overflow: "hidden",
                      "text-overflow": "ellipsis",
                      "white-space": "nowrap",
                    }}
                    title={s.message}
                  >
                    {s.message}
                  </span>
                  <button style={smallBtn} onClick={() => stashOp("stash_apply", s.index)}>
                    Apply
                  </button>
                  <button style={smallBtn} onClick={() => stashOp("stash_pop", s.index)}>
                    Pop
                  </button>
                  <button
                    style={smallBtn}
                    onClick={() => confirm(`Drop stash@{${s.index}}?`) && stashOp("stash_drop", s.index)}
                  >
                    Drop
                  </button>
                </li>
              )}
            </For>
          </ul>
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
        {commitBox()}
      </div>

      {/* Right: the diff for the selected file. */}
      <Show when={selectedSpec()}>
        <div style={{ flex: "1", "min-width": "0", "border-left": "1px solid var(--border)", overflow: "hidden" }}>
          <DiffView
            repoId={props.repoId}
            spec={selectedSpec()!}
            hunkActionLabel={selected()!.staged ? "Unstage hunk" : "Stage hunk"}
            onHunkAction={selected()!.staged ? unstageHunk : stageHunk}
            onStageLines={selected()!.staged ? undefined : stageLines}
            onDiscardLines={selected()!.staged ? undefined : discardLines}
            onDiscardHunk={selected()!.staged ? undefined : discardHunk}
          />
        </div>
      </Show>
    </div>
  );
};

export default ChangesView;
