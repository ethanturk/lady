import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { Component, JSX } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { CommitMeta, FileDiff, FileDiffKind, RefInfo, RepoId, SignatureStatus, WalkLogQuery } from "./commands";
import DiffView from "./DiffView";
import SignatureBadge from "./SignatureBadge";
import { authorColor, initials } from "./avatar";

type Tab = "commit" | "changes" | "filetree";

const FILE_BADGE: Record<FileDiffKind, { label: string; bg: string }> = {
  Added: { label: "A", bg: "var(--badge-a)" },
  Deleted: { label: "D", bg: "var(--badge-d)" },
  Modified: { label: "M", bg: "var(--badge-m)" },
  Binary: { label: "B", bg: "var(--tx4)" },
  Image: { label: "I", bg: "var(--badge-r)" },
};

const FileBadge: Component<{ kind: FileDiffKind }> = (props) => (
  <span
    style={{
      display: "inline-flex",
      "align-items": "center",
      "justify-content": "center",
      width: "17px",
      height: "17px",
      "flex-shrink": "0",
      "border-radius": "4px",
      "font-family": "ui-monospace, monospace",
      "font-weight": 700,
      "font-size": "10px",
      background: FILE_BADGE[props.kind].bg,
      color: "var(--on-badge)",
    }}
    title={props.kind}
  >
    {FILE_BADGE[props.kind].label}
  </span>
);

interface CommitDetailProps {
  repoId: RepoId;
  sha: string;
  selectedShas?: string[];
  refs: RefInfo[];
  sig?: SignatureStatus;
  onCherryPick: () => void;
  onRevert: () => void;
  onRebaseInteractive: (oid: string) => void;
  /** Recompose this commit → HEAD into fewer logical commits (AI). */
  onRecompose: (oid: string) => void;
}

/** Build a directory tree of "dir/ rows + · file rows" from flat paths. */
function treeRows(paths: string[]): { depth: number; label: string; dir: boolean; path?: string }[] {
  const out: { depth: number; label: string; dir: boolean; path?: string }[] = [];
  const seen = new Set<string>();
  for (const p of [...paths].sort()) {
    const parts = p.split("/");
    for (let i = 0; i < parts.length - 1; i++) {
      const key = parts.slice(0, i + 1).join("/");
      if (!seen.has(key)) {
        seen.add(key);
        out.push({ depth: i, label: parts[i], dir: true });
      }
    }
    out.push({ depth: parts.length - 1, label: parts[parts.length - 1], dir: false, path: p });
  }
  return out;
}

/**
 * The bottom detail pane of the All Commits view: Commit / Changes / File Tree
 * tabs. Commit metadata comes from walk_log (start=sha, limit=1); the changed
 * file list comes from diff(commit). Note: the backend's CommitMeta has no full
 * message body, so only the subject is shown.
 */
const CommitDetail: Component<CommitDetailProps> = (props) => {
  const [tab, setTab] = createSignal<Tab>("commit");
  const [meta, setMeta] = createSignal<CommitMeta | null>(null);
  const [files, setFiles] = createSignal<FileDiff[]>([]);
  const [scrollPath, setScrollPath] = createSignal<string | undefined>(undefined);
  const [scrollNonce, setScrollNonce] = createSignal(0);
  let requestId = 0;

  const detailShas = createMemo(() => (props.selectedShas?.length ? props.selectedShas : [props.sha]));
  const uniqueFiles = createMemo(() => {
    const byPath = new Map<string, FileDiff>();
    for (const file of files()) {
      const key = `${file.old_path ?? ""}\0${file.path}`;
      if (!byPath.has(key)) byPath.set(key, file);
    }
    return [...byPath.values()].sort((a, b) => a.path.localeCompare(b.path));
  });

  createEffect(() => {
    const sha = props.sha;
    const repo = props.repoId;
    const shas = detailShas();
    const currentRequest = ++requestId;
    setMeta(null);
    setFiles([]);
    const q: WalkLogQuery = { start: sha, limit: 1 };
    invoke<CommitMeta[]>("walk_log", { repo, query: q })
      .then((m) => {
        if (currentRequest === requestId) setMeta(m[0] ?? null);
      })
      .catch(() => {
        if (currentRequest === requestId) setMeta(null);
      });
    Promise.all(
      shas.map((commit) =>
        invoke<FileDiff[]>("diff", { repo, commit })
          .then((diffs) => diffs.map((file) => ({ ...file, sourceLabel: shas.length > 1 ? commit.slice(0, 8) : undefined })))
          .catch(() => []),
      ),
    ).then((groups) => {
      if (currentRequest === requestId) setFiles(groups.flat());
    });
  });

  const commitRefs = createMemo(() => props.refs.filter((r) => r.target === props.sha && r.kind !== "Head"));
  const fullDate = () => {
    const t = meta()?.time;
    return t ? new Date(t * 1000).toLocaleString() : "";
  };

  const tabBtn = (t: Tab, label: string) => {
    const on = () => tab() === t;
    return (
      <button
        onClick={() => setTab(t)}
        style={{
          position: "relative",
          padding: "0 14px",
          height: "100%",
          border: "none",
          background: "transparent",
          color: on() ? "var(--tx)" : "var(--tx2)",
          "font-size": "12.5px",
          cursor: "pointer",
        }}
      >
        {label}
        <Show when={on()}>
          <span style={{ position: "absolute", bottom: "-1px", left: "8px", right: "8px", height: "2px", background: "var(--accent)" }} />
        </Show>
      </button>
    );
  };

  const metaRow = (label: string, value: JSX.Element) => (
    <>
      <span style={{ color: "var(--tx4)" }}>{label}</span>
      <span style={{ color: "var(--tx2)", "min-width": "0" }}>{value}</span>
    </>
  );

  const openFileInChanges = (path: string) => {
    setScrollPath(path);
    setScrollNonce((n) => n + 1);
    setTab("changes");
  };

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column", background: "var(--panel)", "border-top": "1px solid var(--bd)" }}>
      {/* Tab bar */}
      <div style={{ display: "flex", "align-items": "stretch", height: "38px", "flex-shrink": 0, "border-bottom": "1px solid var(--bd)" }}>
        {tabBtn("commit", "Commit")}
        {tabBtn("changes", "Changes")}
        {tabBtn("filetree", "File Tree")}
        <span style={{ flex: "1" }} />
        <div style={{ display: "flex", "align-items": "center", gap: "6px", padding: "0 10px" }}>
          <button style={btn} onClick={props.onCherryPick}>Cherry-pick</button>
          <button style={btn} onClick={props.onRevert}>Revert</button>
          <button style={btn} onClick={() => props.onRebaseInteractive(props.sha)}>Rebase i</button>
          <button style={btn} title="Recompose this commit → HEAD into fewer logical commits (AI)" onClick={() => props.onRecompose(props.sha)}>Recompose</button>
          <SignatureBadge status={props.sig} />
        </div>
      </div>

      {/* Commit tab */}
      <Show when={tab() === "commit"}>
        <div class="scroll-thin" style={{ flex: "1", "overflow-y": "auto", padding: "20px 26px" }}>
          <Show when={meta()} fallback={<p style={{ color: "var(--tx3)", "font-size": "13px" }}>Loading…</p>}>
            {(m) => (
              <>
                <div style={{ "font-size": "10.5px", "text-transform": "uppercase", "letter-spacing": "0.05em", color: "var(--tx4)", "margin-bottom": "8px" }}>Author</div>
                <div style={{ display: "flex", "align-items": "center", gap: "12px", "margin-bottom": "16px" }}>
                  <span
                    style={{
                      width: "38px",
                      height: "38px",
                      "border-radius": "50%",
                      background: authorColor(m().author.name),
                      color: "#0c0d10",
                      display: "inline-flex",
                      "align-items": "center",
                      "justify-content": "center",
                      "font-size": "14px",
                      "font-weight": 700,
                    }}
                  >
                    {initials(m().author.name)}
                  </span>
                  <div>
                    <div style={{ "font-size": "14px", "font-weight": 600, color: "var(--tx)" }}>{m().author.name}</div>
                    <div style={{ "font-size": "12.5px", color: "var(--tx3)" }}>{m().author.email}</div>
                    <div style={{ "font-size": "12.5px", color: "var(--tx3)" }}>{fullDate()}</div>
                  </div>
                </div>

                <div style={{ display: "grid", "grid-template-columns": "78px 1fr", "row-gap": "9px", "font-size": "12.5px", "align-items": "start", "margin-bottom": "16px" }}>
                  {metaRow(
                    "Refs",
                    <span style={{ display: "flex", gap: "6px", "flex-wrap": "wrap" }}>
                      <For each={commitRefs()} fallback={<span style={{ color: "var(--tx4)" }}>—</span>}>
                        {(r) => (
                          <span style={{ "font-size": "11px", "font-weight": 600, "border-radius": "4px", padding: "1px 7px", border: "1px solid var(--bd)", color: "var(--tx2)", background: "var(--hov)" }}>{r.name}</span>
                        )}
                      </For>
                    </span>,
                  )}
                  {metaRow("SHA", <span style={{ "font-family": "ui-monospace, monospace", "word-break": "break-all" }}>{m().oid}</span>)}
                  {metaRow(
                    "Parents",
                    <span style={{ display: "flex", gap: "8px", "flex-wrap": "wrap" }}>
                      <For each={m().parents} fallback={<span style={{ color: "var(--tx4)" }}>none</span>}>
                        {(p) => <span style={{ "font-family": "ui-monospace, monospace", color: "var(--accent)" }}>{p.slice(0, 8)}</span>}
                      </For>
                    </span>,
                  )}
                </div>

                <div style={{ "font-size": "15px", "font-weight": 600, color: "var(--tx)", "margin-bottom": "16px" }}>{m().summary}</div>

                {/* Files changed footer */}
                <div style={{ "font-size": "10.5px", "text-transform": "uppercase", "letter-spacing": "0.05em", color: "var(--tx4)", "margin-bottom": "6px" }}>
                  {uniqueFiles().length} file{uniqueFiles().length === 1 ? "" : "s"} changed
                  <Show when={detailShas().length > 1}> across {detailShas().length} commits</Show>
                </div>
                <For each={uniqueFiles()}>
                  {(f) => (
                    <div class="hov" style={{ display: "flex", "align-items": "center", gap: "8px", padding: "4px 6px", "border-radius": "6px", "font-size": "12.5px" }}>
                      <FileBadge kind={f.kind} />
                      <span style={{ "font-family": "ui-monospace, monospace", color: "var(--tx2)", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }} title={f.path}>{f.path}</span>
                    </div>
                  )}
                </For>
              </>
            )}
          </Show>
        </div>
      </Show>

      {/* Changes tab */}
      <Show when={tab() === "changes"}>
        <div style={{ flex: "1", "min-height": "0" }}>
          <DiffView
            repoId={props.repoId}
            commit={detailShas().length === 1 ? props.sha : undefined}
            files={files()}
            title={detailShas().length > 1 ? `${detailShas().length} selected commits` : undefined}
            collapsible
            scrollToPath={scrollPath()}
            scrollKey={scrollNonce()}
          />
        </div>
      </Show>

      {/* File Tree tab */}
      <Show when={tab() === "filetree"}>
        <div class="scroll-thin" style={{ flex: "1", "overflow-y": "auto", padding: "12px 0", "font-family": "ui-monospace, monospace", "font-size": "12.5px" }}>
          <For each={treeRows(uniqueFiles().map((f) => f.path))}>
            {(node) => (
              <div
                class="hov"
                onClick={() => node.path && openFileInChanges(node.path)}
                style={{
                  padding: "2px 0",
                  "padding-left": `${14 + node.depth * 18}px`,
                  color: node.dir ? "var(--tx3)" : "var(--tx2)",
                  cursor: node.path ? "pointer" : "default",
                  "user-select": "none",
                }}
              >
                {node.dir ? "▾ " : "· "}
                {node.label}
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

const btn = {
  border: "1px solid var(--bd)",
  background: "var(--btn)",
  color: "var(--tx)",
  "border-radius": "6px",
  "font-size": "11px",
  padding: "3px 9px",
  cursor: "pointer",
} as const;

export default CommitDetail;
