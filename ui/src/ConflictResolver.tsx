import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type {
  ConflictRegion,
  ConflictSegment,
  ConflictSides,
  ConflictState,
  ParsedConflict,
  RebaseOutcome,
  RepoId,
} from "./commands";

/** One conflict region's working resolution: which side, and the edited text. */
interface RegionState {
  choice: "ours" | "theirs" | "both" | null;
  text: string;
}

const OURS_BG = "#e6ffec";
const THEIRS_BG = "#ddf4ff";
const BASE_BG = "#f6f8fa";

const pane = {
  flex: "1",
  "min-width": "0",
  overflow: "auto",
  "font-family": "monospace",
  "font-size": "0.78rem",
  "white-space": "pre-wrap" as const,
  padding: "0.4rem 0.6rem",
};

const sideText = (s: string | null) => (s == null ? "(absent)" : s);

/** Join the chosen lines of a region for an initial editable resolution. */
function regionInitial(region: ConflictRegion, choice: RegionState["choice"]): string {
  if (choice === "ours") return region.ours.join("\n");
  if (choice === "theirs") return region.theirs.join("\n");
  if (choice === "both") return [...region.ours, ...region.theirs].join("\n");
  return "";
}

/**
 * 3-pane merge conflict resolver (PH3-002). Walks the repo's conflicted files
 * one at a time: base | ours | theirs read-only panes plus an editable,
 * per-region result. Saving writes the resolution and marks the file resolved
 * (PH3-001 commands), then advances. Completing all files calls `onDone`.
 */
const ConflictResolver: Component<{
  repoId: RepoId;
  refreshNonce: number;
  conflictState: ConflictState;
  onChanged: () => void;
  onDone: () => void;
}> = (props) => {
  const [paths, setPaths] = createSignal<string[]>([]);
  const [idx, setIdx] = createSignal(0);
  const [sides, setSides] = createSignal<ConflictSides>({ base: null, ours: null, theirs: null });
  const [parsed, setParsed] = createSignal<ParsedConflict | null>(null);
  const [regions, setRegions] = createSignal<RegionState[]>([]);
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const current = () => paths()[idx()];

  // Reload the conflict list whenever the repo or refresh nonce changes.
  createEffect(() => {
    props.refreshNonce;
    const repo = props.repoId;
    invoke<string[]>("list_conflicts", { repo })
      .then((p) => {
        setPaths(p);
        if (idx() >= p.length) setIdx(0);
      })
      .catch((e) => setErr(String(e)));
  });

  // Load the current file's sides + parsed regions.
  createEffect(() => {
    const path = current();
    const repo = props.repoId;
    if (!path) {
      setParsed(null);
      return;
    }
    setErr(null);
    invoke<ConflictSides>("conflict_sides", { repo, path }).then(setSides).catch((e) => setErr(String(e)));
    invoke<ParsedConflict>("parse_conflict", { repo, path })
      .then((p) => {
        setParsed(p);
        // One RegionState per conflict segment, defaulting to "ours".
        const rs: RegionState[] = [];
        for (const seg of p.segments) {
          if (seg.kind === "Conflict") rs.push({ choice: "ours", text: regionInitial(seg.value, "ours") });
        }
        setRegions(rs);
      })
      .catch((e) => setErr(String(e)));
  });

  // Conflict-region positions among segments, for region indexing + minimap.
  const conflictSegments = (): { seg: ConflictSegment; regionIndex: number }[] => {
    const out: { seg: ConflictSegment; regionIndex: number }[] = [];
    let r = 0;
    for (const seg of parsed()?.segments ?? []) {
      out.push({ seg, regionIndex: seg.kind === "Conflict" ? r++ : -1 });
    }
    return out;
  };

  const setRegion = (i: number, patch: Partial<RegionState>) =>
    setRegions((prev) => prev.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  const choose = (i: number, choice: RegionState["choice"], region: ConflictRegion) =>
    setRegion(i, { choice, text: regionInitial(region, choice) });

  const allChosen = () => regions().every((r) => r.choice !== null);

  /** Assemble the resolved file from context lines + per-region edited text. */
  const buildResolution = (): string => {
    const lines: string[] = [];
    let r = 0;
    for (const seg of parsed()?.segments ?? []) {
      if (seg.kind === "Context") {
        for (const l of seg.value) lines.push(l);
      } else {
        const text = regions()[r]?.text ?? "";
        if (text.length > 0) for (const l of text.split("\n")) lines.push(l);
        r++;
      }
    }
    return lines.length ? lines.join("\n") + "\n" : "";
  };

  const advance = () => {
    setBusy(true);
    props.onChanged();
    // After onChanged the parent bumps the nonce; reload the list to see if
    // anything remains, then either move on or finish.
    invoke<string[]>("list_conflicts", { repo: props.repoId })
      .then((p) => {
        setPaths(p);
        if (p.length === 0) {
          props.onDone();
        } else if (idx() >= p.length) {
          setIdx(0);
        }
      })
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  };

  const saveResolution = () => {
    const path = current();
    if (!path) return;
    setErr(null);
    setBusy(true);
    const content = buildResolution();
    invoke("write_resolution", { repo: props.repoId, path, content })
      .then(() => invoke("mark_resolved", { repo: props.repoId, path }))
      .then(advance)
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  // Whole-file shortcuts via the engine (PH3-001 take_ours / take_theirs).
  const takeWholeFile = (cmd: "take_ours" | "take_theirs") => {
    const path = current();
    if (!path) return;
    setErr(null);
    setBusy(true);
    invoke(cmd, { repo: props.repoId, path })
      .then(() => invoke("mark_resolved", { repo: props.repoId, path }))
      .then(advance)
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  // Rebase sequencing controls (PH3-004 handoff). Continue/skip drive the
  // engine; the parent is told to finish when the rebase completes.
  const runRebaseStep = (cmd: "rebase_continue" | "rebase_skip") => {
    setErr(null);
    setBusy(true);
    invoke<RebaseOutcome>(cmd, { repo: props.repoId })
      .then((outcome) => {
        setBusy(false);
        if (outcome.kind === "Rebased") {
          props.onDone();
        } else {
          // More conflicts (or an edit stop) — reload and keep resolving.
          props.onChanged();
        }
      })
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  const abort = () => {
    setErr(null);
    setBusy(true);
    invoke("conflict_abort", { repo: props.repoId })
      .then(() => {
        setBusy(false);
        props.onDone();
      })
      .catch((e) => {
        setErr(String(e));
        setBusy(false);
      });
  };

  const headerBtn = {
    border: "1px solid #ccc",
    background: "#fff",
    "border-radius": "3px",
    "font-size": "0.75rem",
    cursor: "pointer",
    padding: "0.2rem 0.5rem",
  };

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column" }}>
      <Show
        when={paths().length > 0}
        fallback={
          <p style={{ color: "#1a7f37", padding: "1rem", "font-size": "0.9rem" }}>
            <Show
              when={props.conflictState === "Rebase"}
              fallback="No conflicts to resolve. 🎉"
            >
              All files resolved — continue the rebase to finish.
            </Show>
          </p>
        }
      >
        {/* Header: progress + file + whole-file actions */}
        <div
          style={{
            display: "flex",
            "align-items": "center",
            gap: "0.5rem",
            padding: "0.4rem 0.6rem",
            "border-bottom": "1px solid #ddd",
            "flex-shrink": 0,
          }}
        >
          <span style={{ "font-size": "0.8rem", color: "#666" }}>
            Conflict {idx() + 1} / {paths().length}
          </span>
          <span style={{ "font-family": "monospace", "font-size": "0.8rem", "font-weight": 600 }}>
            {current()}
          </span>
          <span style={{ flex: "1" }} />
          <button style={headerBtn} disabled={busy()} onClick={() => takeWholeFile("take_ours")}>
            Use ours
          </button>
          <button style={headerBtn} disabled={busy()} onClick={() => takeWholeFile("take_theirs")}>
            Use theirs
          </button>
          <button
            style={headerBtn}
            disabled={busy()}
            title="Open in external merge tool"
            onClick={() => {
              const path = current();
              if (!path) return;
              setErr(null);
              invoke("launch_mergetool", { repo: props.repoId, path })
                .then(() => props.onChanged())
                .catch((e) => setErr(String(e)));
            }}
          >
            Ext merge
          </button>
          <button
            style={{ ...headerBtn, background: allChosen() ? "#1a7f37" : "#eee", color: allChosen() ? "#fff" : "#999" }}
            disabled={busy() || !allChosen()}
            onClick={saveResolution}
          >
            Save &amp; next
          </button>
        </div>

        <Show when={err()}>
          <p style={{ color: "crimson", margin: "0.25rem 0.6rem", "font-size": "0.85rem" }}>{err()}</p>
        </Show>

        {/* Three read-only panes: base | ours | theirs */}
        <div style={{ display: "flex", height: "32%", "border-bottom": "1px solid #ddd", "flex-shrink": 0 }}>
          <div style={{ ...pane, background: BASE_BG, "border-right": "1px solid #eee" }}>
            <div style={{ color: "#888", "font-weight": 700, "margin-bottom": "0.25rem" }}>BASE</div>
            {sideText(sides().base)}
          </div>
          <div style={{ ...pane, background: OURS_BG, "border-right": "1px solid #eee" }}>
            <div style={{ color: "#1a7f37", "font-weight": 700, "margin-bottom": "0.25rem" }}>OURS</div>
            {sideText(sides().ours)}
          </div>
          <div style={{ ...pane, background: THEIRS_BG }}>
            <div style={{ color: "#0969da", "font-weight": 700, "margin-bottom": "0.25rem" }}>THEIRS</div>
            {sideText(sides().theirs)}
          </div>
        </div>

        {/* Editable result: context lines + per-region choose/edit. A minimap
            strip on the right marks each conflict region. */}
        <div style={{ flex: "1", display: "flex", "min-height": "0" }}>
          <div style={{ flex: "1", overflow: "auto", padding: "0.5rem 0.6rem" }}>
            <div style={{ color: "#666", "font-size": "0.75rem", "margin-bottom": "0.4rem" }}>
              RESULT — choose a side per conflict, then edit if needed
            </div>
            <For each={conflictSegments()}>
              {(item) => (
                <Show
                  when={item.seg.kind === "Conflict"}
                  fallback={
                    <pre
                      style={{
                        margin: 0,
                        "font-family": "monospace",
                        "font-size": "0.78rem",
                        color: "#444",
                        "white-space": "pre-wrap",
                      }}
                    >
                      {(item.seg as { value: string[] }).value.join("\n")}
                    </pre>
                  }
                >
                  {(() => {
                    const region = (item.seg as { value: ConflictRegion }).value;
                    const ri = item.regionIndex;
                    const choiceBtn = (c: RegionState["choice"], label: string, bg: string) => (
                      <button
                        onClick={() => choose(ri, c, region)}
                        style={{
                          border: "1px solid #ccc",
                          "border-radius": "3px",
                          "font-size": "0.7rem",
                          cursor: "pointer",
                          padding: "0.1rem 0.4rem",
                          background: regions()[ri]?.choice === c ? bg : "#fff",
                          "font-weight": regions()[ri]?.choice === c ? 700 : 400,
                        }}
                      >
                        {label}
                      </button>
                    );
                    return (
                      <div
                        style={{
                          border: "1px solid #f0c36d",
                          "border-radius": "4px",
                          margin: "0.3rem 0",
                          background: "#fffdf7",
                        }}
                      >
                        <div style={{ display: "flex", gap: "0.3rem", padding: "0.25rem 0.4rem" }}>
                          <span style={{ "font-size": "0.7rem", color: "#999", flex: "1" }}>
                            conflict #{ri + 1}
                          </span>
                          {choiceBtn("ours", "Ours", OURS_BG)}
                          {choiceBtn("theirs", "Theirs", THEIRS_BG)}
                          {choiceBtn("both", "Both", "#f0f3f6")}
                        </div>
                        <textarea
                          value={regions()[ri]?.text ?? ""}
                          onInput={(e) => setRegion(ri, { text: e.currentTarget.value })}
                          spellcheck={false}
                          style={{
                            width: "100%",
                            "box-sizing": "border-box",
                            "font-family": "monospace",
                            "font-size": "0.78rem",
                            border: "none",
                            "border-top": "1px solid #f0e3c0",
                            background: "transparent",
                            padding: "0.3rem 0.4rem",
                            "min-height": "3.2rem",
                            resize: "vertical",
                          }}
                        />
                      </div>
                    );
                  })()}
                </Show>
              )}
            </For>
          </div>

          {/* Minimap markers on the scrollbar edge. */}
          <div
            style={{
              width: "10px",
              "flex-shrink": 0,
              background: "#f6f8fa",
              "border-left": "1px solid #eee",
              position: "relative",
            }}
            title="conflict markers"
          >
            <For each={regions()}>
              {(r, i) => (
                <div
                  style={{
                    position: "absolute",
                    left: "1px",
                    width: "8px",
                    height: "6px",
                    top: `${regions().length ? (i() / regions().length) * 100 : 0}%`,
                    background: r.choice ? "#1a7f37" : "#d1242f",
                    "border-radius": "2px",
                  }}
                />
              )}
            </For>
          </div>
        </div>
      </Show>

      {/* Sequencing footer: continue/skip a rebase, abort any operation. */}
      <Show when={props.conflictState !== "None"}>
        <div
          style={{
            display: "flex",
            gap: "0.5rem",
            padding: "0.4rem 0.6rem",
            "border-top": "1px solid #ddd",
            "flex-shrink": 0,
            "align-items": "center",
          }}
        >
          <span style={{ "font-size": "0.78rem", color: "#666" }}>
            In {props.conflictState.toLowerCase()}
          </span>
          <span style={{ flex: "1" }} />
          <Show when={props.conflictState === "Rebase"}>
            <button style={headerBtn} disabled={busy()} onClick={() => runRebaseStep("rebase_continue")}>
              Continue rebase
            </button>
            <button style={headerBtn} disabled={busy()} onClick={() => runRebaseStep("rebase_skip")}>
              Skip commit
            </button>
          </Show>
          <button
            style={{ ...headerBtn, color: "#d1242f", "border-color": "#d1242f" }}
            disabled={busy()}
            onClick={abort}
          >
            Abort
          </button>
        </div>
      </Show>
    </div>
  );
};

export default ConflictResolver;
