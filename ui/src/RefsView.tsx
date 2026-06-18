import { createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { FfMode, MergeOutcome, RefInfo, RefKind, RepoId } from "./commands";

interface RefsViewProps {
  repoId: RepoId;
  refs: RefInfo[];
  /** Called after a mutation so App reloads refs + status + graph. */
  onChanged: () => void;
}

const smallBtn = {
  border: "1px solid #ccc",
  background: "#fff",
  "border-radius": "3px",
  "font-size": "0.7rem",
  padding: "0 0.4rem",
  cursor: "pointer",
};

/**
 * The Refs view: branches, tags, and remotes with create / delete / checkout
 * actions. The branch whose tip matches HEAD is marked current (●). Destructive
 * actions (delete, force checkout) require explicit confirmation.
 */
const RefsView: Component<RefsViewProps> = (props) => {
  const [err, setErr] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [conflicts, setConflicts] = createSignal<string[]>([]);
  const [newBranch, setNewBranch] = createSignal("");
  const [newTag, setNewTag] = createSignal("");
  const [ffMode, setFfMode] = createSignal<FfMode>("Auto");
  const [mergeMessage, setMergeMessage] = createSignal("");

  const byKind = (kind: RefKind) => props.refs.filter((r) => r.kind === kind);
  const headTarget = () => props.refs.find((r) => r.kind === "Head")?.target;
  const isCurrent = (r: RefInfo) => r.kind === "Branch" && r.target === headTarget();

  const run = (cmd: string, args: Record<string, unknown>) => {
    setErr(null);
    setNotice(null);
    invoke(cmd, { repo: props.repoId, ...args })
      .then(() => props.onChanged())
      .catch((e) => setErr(String(e)));
  };

  const checkout = (target: string, force = false) =>
    run("checkout", { target, force });

  const checkoutSafely = (target: string) => {
    setErr(null);
    invoke("checkout", { repo: props.repoId, target, force: false })
      .then(() => props.onChanged())
      .catch((e) => {
        // Offer a force checkout only after git refused (e.g. local changes).
        if (confirm(`${e}\n\nForce checkout '${target}' and discard local changes?`)) {
          checkout(target, true);
        } else {
          setErr(String(e));
        }
      });
  };

  const deleteBranch = (name: string) => {
    if (!confirm(`Delete branch '${name}'?`)) return;
    setErr(null);
    invoke("delete_branch", { repo: props.repoId, name, force: false })
      .then(() => props.onChanged())
      .catch((e) => {
        // Unmerged branch → git refuses with -d; offer the forceful -D.
        if (confirm(`${e}\n\nForce-delete '${name}' anyway?`)) {
          run("delete_branch", { name, force: true });
        } else {
          setErr(String(e));
        }
      });
  };

  const deleteTag = (name: string) => {
    if (!confirm(`Delete tag '${name}'?`)) return;
    run("delete_tag", { name });
  };

  const createBranch = () => {
    const name = newBranch().trim();
    if (!name) return;
    run("create_branch", { name, startPoint: null });
    setNewBranch("");
  };

  const createTag = () => {
    const name = newTag().trim();
    if (!name) return;
    run("create_tag", { name, target: null, message: null });
    setNewTag("");
  };

  const describeMerge = (outcome: MergeOutcome) => {
    switch (outcome.kind) {
      case "AlreadyUpToDate":
        return "Already up to date.";
      case "FastForwarded":
        return "Fast-forwarded.";
      case "Merged":
        return `Merged as ${outcome.value.slice(0, 8)}.`;
      case "Conflicts":
        return `Merge stopped with ${outcome.value.length} conflict${outcome.value.length === 1 ? "" : "s"}.`;
    }
  };

  const mergeSource = (source: string) => {
    setErr(null);
    setNotice(null);
    setConflicts([]);
    const message = mergeMessage().trim();
    invoke<MergeOutcome>("merge", {
      repo: props.repoId,
      source,
      fastForward: ffMode(),
      commitMessage: message ? message : null,
    })
      .then((outcome) => {
        if (outcome.kind === "Conflicts") {
          setConflicts(outcome.value);
        }
        setNotice(describeMerge(outcome));
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const abortMerge = () => {
    setErr(null);
    invoke("merge_abort", { repo: props.repoId })
      .then(() => {
        setConflicts([]);
        setNotice("Merge aborted.");
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const inputStyle = {
    border: "1px solid #ccc",
    "border-radius": "3px",
    "font-size": "0.75rem",
    padding: "0.1rem 0.3rem",
  };

  return (
    <div style={{ padding: "0.5rem 1rem", "overflow-y": "auto", height: "100%" }}>
      <Show when={err()}>
        <p style={{ color: "crimson", "font-size": "0.8rem", "white-space": "pre-wrap" }}>{err()}</p>
      </Show>
      <Show when={notice()}>
        <p style={{ color: "#1a7f37", "font-size": "0.8rem", "white-space": "pre-wrap" }}>{notice()}</p>
      </Show>
      <Show when={conflicts().length > 0}>
        <div style={{ border: "1px solid #f0c36d", background: "#fff8e5", padding: "0.4rem", "font-size": "0.8rem" }}>
          <div style={{ "font-weight": 700, "margin-bottom": "0.25rem" }}>Conflicted files</div>
          <ul style={{ margin: 0, padding: "0 0 0 1.2rem" }}>
            <For each={conflicts()}>{(path) => <li>{path}</li>}</For>
          </ul>
          <button style={{ ...smallBtn, "margin-top": "0.35rem" }} onClick={abortMerge}>
            Abort merge
          </button>
        </div>
      </Show>

      <section>
        <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", margin: "0.5rem 0 0.25rem" }}>
          <h3 style={{ margin: 0, "font-size": "0.9rem" }}>Branches</h3>
          <select
            style={inputStyle}
            value={ffMode()}
            onChange={(e) => setFfMode(e.currentTarget.value as FfMode)}
            title="Merge fast-forward policy"
          >
            <option value="Auto">FF auto</option>
            <option value="Only">FF only</option>
            <option value="Never">No FF</option>
          </select>
          <input
            style={{ ...inputStyle, width: "11rem" }}
            placeholder="merge message"
            value={mergeMessage()}
            onInput={(e) => setMergeMessage(e.currentTarget.value)}
          />
          <input
            style={inputStyle}
            placeholder="new-branch"
            value={newBranch()}
            onInput={(e) => setNewBranch(e.currentTarget.value)}
            onKeyDown={(e) => e.key === "Enter" && createBranch()}
          />
          <button style={smallBtn} onClick={createBranch}>
            + Branch
          </button>
        </div>
        <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
          <For each={byKind("Branch")}>
            {(r) => (
              <li
                style={{
                  display: "flex",
                  "align-items": "center",
                  gap: "0.4rem",
                  "font-family": "monospace",
                  "font-size": "0.85rem",
                  padding: "0.1rem 0",
                }}
              >
                <span style={{ width: "0.8rem", color: "#1a7f37" }}>{isCurrent(r) ? "●" : ""}</span>
                <span style={{ flex: "1", "font-weight": isCurrent(r) ? 700 : 400 }}>{r.name}</span>
                <span style={{ color: "#888" }}>{r.target.slice(0, 8)}</span>
                <Show when={!isCurrent(r)}>
                  <button style={smallBtn} onClick={() => checkoutSafely(r.name)}>
                    Checkout
                  </button>
                  <button style={smallBtn} onClick={() => mergeSource(r.name)}>
                    Merge
                  </button>
                  <button style={smallBtn} onClick={() => deleteBranch(r.name)}>
                    Delete
                  </button>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </section>

      <section>
        <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", margin: "0.75rem 0 0.25rem" }}>
          <h3 style={{ margin: 0, "font-size": "0.9rem" }}>Tags</h3>
          <input
            style={inputStyle}
            placeholder="new-tag"
            value={newTag()}
            onInput={(e) => setNewTag(e.currentTarget.value)}
            onKeyDown={(e) => e.key === "Enter" && createTag()}
          />
          <button style={smallBtn} onClick={createTag}>
            + Tag
          </button>
        </div>
        <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
          <For each={byKind("Tag")}>
            {(r) => (
              <li
                style={{
                  display: "flex",
                  "align-items": "center",
                  gap: "0.4rem",
                  "font-family": "monospace",
                  "font-size": "0.85rem",
                  padding: "0.1rem 0",
                }}
              >
                <span style={{ width: "0.8rem" }} />
                <span style={{ flex: "1" }}>{r.name}</span>
                <span style={{ color: "#888" }}>{r.target.slice(0, 8)}</span>
                <button style={smallBtn} onClick={() => checkout(r.name)}>
                  Checkout
                </button>
                <button style={smallBtn} onClick={() => deleteTag(r.name)}>
                  Delete
                </button>
              </li>
            )}
          </For>
        </ul>
      </section>

      <Show when={byKind("Remote").length > 0}>
        <section>
          <h3 style={{ margin: "0.75rem 0 0.25rem", "font-size": "0.9rem" }}>Remotes</h3>
          <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
            <For each={byKind("Remote")}>
              {(r) => (
                <li
                  style={{
                    display: "flex",
                    "align-items": "center",
                    gap: "0.4rem",
                    "font-family": "monospace",
                    "font-size": "0.85rem",
                    padding: "0.1rem 0",
                  }}
                >
                  <span style={{ width: "0.8rem" }} />
                  <span style={{ flex: "1" }}>{r.name}</span>
                  <span style={{ color: "#888" }}>{r.target.slice(0, 8)}</span>
                  <button style={smallBtn} onClick={() => checkoutSafely(r.name)}>
                    Checkout
                  </button>
                  <button style={smallBtn} onClick={() => mergeSource(r.name)}>
                    Merge
                  </button>
                </li>
              )}
            </For>
          </ul>
        </section>
      </Show>
    </div>
  );
};

export default RefsView;
