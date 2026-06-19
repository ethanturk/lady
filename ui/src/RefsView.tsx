import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type {
  FfMode,
  ForgeKind,
  HostingInfo,
  MergeOutcome,
  RebaseOutcome,
  RefInfo,
  RefKind,
  RepoId,
} from "./commands";
import type { ResolvedRepoSettings } from "./commands";
import { requestNoun } from "./commands";
import { isConsentError, runAiStream } from "./ai";
import { effectiveFf, repoSettings } from "./repoSettings";

interface RefsViewProps {
  repoId: RepoId;
  refs: RefInfo[];
  /** Called after a mutation so App reloads refs + status + graph. */
  onChanged: () => void;
  /** Open the interactive-rebase editor from a branch tip (PH3-004). */
  onInteractiveRebase: (oid: string) => void;
}

const smallBtn = {
  border: "1px solid var(--border)",
  background: "var(--surface)",
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
  const [abortCmd, setAbortCmd] = createSignal<"merge_abort" | "rebase_abort">("merge_abort");
  const [newBranch, setNewBranch] = createSignal("");
  const [newTag, setNewTag] = createSignal("");
  const [ffMode, setFfMode] = createSignal<FfMode>("Auto");
  // Effective settings (per-repo override ?? global default) — seeds the merge
  // fast-forward dropdown and the default base branch when the repo changes.
  const [effSettings, setEffSettings] = createSignal<ResolvedRepoSettings | null>(null);
  createEffect(() => {
    const repo = props.repoId;
    repoSettings(repo)
      .then((r) => {
        setEffSettings(r);
        setFfMode(effectiveFf(r));
      })
      .catch(() => {});
  });
  const [mergeMessage, setMergeMessage] = createSignal("");

  // The active repo's forge, for PR/MR labelling (PH4-002).
  const [forge, setForge] = createSignal<ForgeKind | null>(null);
  createEffect(() => {
    invoke<HostingInfo>("hosting_status", { repo: props.repoId })
      .then((s) => setForge(s.kind))
      .catch(() => setForge(null));
  });
  const noun = () => requestNoun(forge());
  const nounTitle = () => noun().replace(/\b\w/g, (c) => c.toUpperCase());

  // Create-PR form state (PH3-012).
  const [prBranch, setPrBranch] = createSignal<string | null>(null);
  const [prTitle, setPrTitle] = createSignal("");
  const [prBody, setPrBody] = createSignal("");
  const [prBase, setPrBase] = createSignal("");
  const [prDraft, setPrDraft] = createSignal(false);
  const [prUrl, setPrUrl] = createSignal<string | null>(null);
  const [prBusy, setPrBusy] = createSignal(false);
  const [aiBusy, setAiBusy] = createSignal(false);

  // Generate the PR/MR title or description with AI (PH5-010), streaming into
  // the field. Respects consent + the per-repo toggle (enforced server-side).
  const aiPrText = async (which: "title" | "description") => {
    const head = prBranch();
    const base = prBase().trim();
    if (!head || !base || aiBusy()) return;
    setErr(null);
    setAiBusy(true);
    const setter = which === "title" ? setPrTitle : setPrBody;
    setter("");
    try {
      const cmd = which === "title" ? "ai_pr_title" : "ai_pr_description";
      const full = await runAiStream(
        cmd,
        { repo: props.repoId, base, head },
        (acc) => setter(acc),
      );
      setter(full);
    } catch (e) {
      const msg = String(e);
      setErr(
        isConsentError(msg)
          ? "AI consent required — enable the provider and grant consent in Settings."
          : msg,
      );
    } finally {
      setAiBusy(false);
    }
  };

  // The repo's default base branch for prefill: the configured (override ??
  // global) base when it exists as a branch, else the main/master auto-guess.
  const defaultBase = () => {
    const names = props.refs.filter((r) => r.kind === "Branch").map((r) => r.name);
    const configured = effSettings()?.effective.base_branch;
    if (configured && names.includes(configured)) return configured;
    return names.find((n) => n === "main") ?? names.find((n) => n === "master") ?? names[0] ?? "main";
  };

  const openPrForm = (branch: string) => {
    setErr(null);
    setPrUrl(null);
    setPrBranch(branch);
    setPrTitle(branch);
    setPrBody("");
    setPrBase(defaultBase());
    setPrDraft(false);
    // Prefill the body with recent commit subjects as a starting point.
    invoke<string[]>("recent_messages", { repo: props.repoId, limit: 5 })
      .then((msgs) => setPrBody(msgs.map((m) => `- ${m}`).join("\n")))
      .catch(() => {});
  };

  const submitPr = () => {
    const head = prBranch();
    if (!head || !prTitle().trim() || !prBase().trim()) return;
    setPrBusy(true);
    setErr(null);
    invoke<string>("github_create_pr", {
      repo: props.repoId,
      head,
      base: prBase().trim(),
      title: prTitle().trim(),
      body: prBody(),
      draft: prDraft(),
    })
      .then((url) => setPrUrl(url))
      .catch((e) => setErr(String(e)))
      .finally(() => setPrBusy(false));
  };

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
    setAbortCmd("merge_abort");
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

  const mergeInto = (source: string, target: string) => {
    const runMerge = () => mergeSource(source);
    const targetRef = byKind("Branch").find((r) => r.name === target);
    if (targetRef && isCurrent(targetRef)) {
      runMerge();
      return;
    }
    setErr(null);
    invoke("checkout", { repo: props.repoId, target, force: false })
      .then(runMerge)
      .catch((e) => setErr(String(e)));
  };

  const describeRebase = (branch: string, onto: string, outcome: RebaseOutcome) => {
    if (outcome.kind === "Rebased") return `Rebased ${branch} onto ${onto}.`;
    if (outcome.kind === "Stopped") return "Rebase stopped (edit step) — continue or abort.";
    return `Rebase stopped with ${outcome.value.length} conflict${outcome.value.length === 1 ? "" : "s"}.`;
  };

  const rebaseBranch = (branch: string, onto: string) => {
    setErr(null);
    setNotice(null);
    setConflicts([]);
    setAbortCmd("rebase_abort");
    invoke<RebaseOutcome>("rebase", { repo: props.repoId, branch, onto })
      .then((outcome) => {
        if (outcome.kind === "Conflicts") setConflicts(outcome.value);
        setNotice(describeRebase(branch, onto, outcome));
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const onBranchDrop = (target: string, ev: DragEvent) => {
    ev.preventDefault();
    const source = ev.dataTransfer?.getData("text/plain") ?? "";
    if (!source || source === target) return;
    const action = prompt(`Drop '${source}' onto '${target}'. Type 'merge' or 'rebase'.`, "merge");
    if (action?.toLowerCase() === "merge") mergeInto(source, target);
    if (action?.toLowerCase() === "rebase") rebaseBranch(source, target);
  };

  const abortOperation = () => {
    setErr(null);
    invoke(abortCmd(), { repo: props.repoId })
      .then(() => {
        setConflicts([]);
        setNotice("Operation aborted.");
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const inputStyle = {
    border: "1px solid var(--border)",
    "border-radius": "3px",
    "font-size": "0.75rem",
    padding: "0.1rem 0.3rem",
  };

  return (
    <div style={{ padding: "0.5rem 1rem", "overflow-y": "auto", height: "100%" }}>
      {/* Create-PR form (PH3-012) */}
      <Show when={prBranch()}>
        <div
          style={{
            position: "fixed",
            inset: "0",
            background: "rgba(0,0,0,0.35)",
            display: "flex",
            "align-items": "center",
            "justify-content": "center",
            "z-index": "50",
          }}
          onClick={() => setPrBranch(null)}
        >
          <div
            style={{
              background: "var(--surface)",
              "border-radius": "6px",
              width: "560px",
              "max-width": "92vw",
              padding: "0.9rem 1rem",
              "box-shadow": "0 8px 30px rgba(0,0,0,0.25)",
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div style={{ "font-weight": 600, "margin-bottom": "0.5rem" }}>
              Create {noun()} — {prBranch()}
            </div>
            <Show when={err()}>
              <p style={{ color: "var(--error)", "font-size": "0.82rem", "white-space": "pre-wrap" }}>{err()}</p>
            </Show>
            <Show
              when={prUrl()}
              fallback={
                <div style={{ display: "flex", "flex-direction": "column", gap: "0.45rem" }}>
                  <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.82rem" }}>
                    <span style={{ width: "3.5rem" }}>base</span>
                    <input style={{ flex: "1", padding: "0.3rem 0.5rem" }} value={prBase()} onInput={(e) => setPrBase(e.currentTarget.value)} />
                  </label>
                  <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.82rem" }}>
                    <span style={{ width: "3.5rem" }}>title</span>
                    <input style={{ flex: "1", padding: "0.3rem 0.5rem" }} value={prTitle()} onInput={(e) => setPrTitle(e.currentTarget.value)} />
                    <button type="button" disabled={aiBusy()} title="Generate title with AI" onClick={() => aiPrText("title")} style={{ "font-size": "0.75rem", padding: "0.2rem 0.5rem" }}>
                      ✨
                    </button>
                  </label>
                  <div style={{ display: "flex", "align-items": "center", gap: "0.4rem" }}>
                    <span style={{ "font-size": "0.78rem", color: "var(--fg-muted, var(--fg-muted))" }}>description</span>
                    <button type="button" disabled={aiBusy()} title="Generate description with AI" onClick={() => aiPrText("description")} style={{ "font-size": "0.75rem", padding: "0.2rem 0.5rem" }}>
                      {aiBusy() ? "Generating…" : "✨ Generate"}
                    </button>
                  </div>
                  <textarea
                    style={{ "min-height": "6rem", padding: "0.3rem 0.5rem", "font-family": "inherit", "font-size": "0.82rem" }}
                    placeholder="Description"
                    value={prBody()}
                    onInput={(e) => setPrBody(e.currentTarget.value)}
                  />
                  <label style={{ display: "flex", "align-items": "center", gap: "0.3rem", "font-size": "0.82rem" }}>
                    <input type="checkbox" checked={prDraft()} onChange={() => setPrDraft((d) => !d)} />
                    Draft
                  </label>
                  <div style={{ display: "flex", gap: "0.5rem", "justify-content": "flex-end" }}>
                    <button onClick={() => setPrBranch(null)}>Cancel</button>
                    <button
                      onClick={submitPr}
                      disabled={prBusy()}
                      style={{ background: "var(--success)", color: "var(--on-accent)", border: "none", "border-radius": "3px", padding: "0.3rem 0.9rem", cursor: "pointer" }}
                    >
                      {prBusy() ? "Creating…" : `Create ${nounTitle()}`}
                    </button>
                  </div>
                </div>
              }
            >
              <div>
                <p style={{ color: "var(--success)", "font-size": "0.85rem" }}>{nounTitle()} created.</p>
                <p style={{ "font-family": "monospace", "font-size": "0.8rem", "word-break": "break-all" }}>{prUrl()}</p>
                <div style={{ display: "flex", gap: "0.5rem", "justify-content": "flex-end" }}>
                  <button onClick={() => setPrBranch(null)}>Close</button>
                  <button
                    onClick={() => invoke("open_url", { url: prUrl() }).catch((e) => setErr(String(e)))}
                    style={{ background: "var(--accent)", color: "var(--on-accent)", border: "none", "border-radius": "3px", padding: "0.3rem 0.9rem", cursor: "pointer" }}
                  >
                    Open in browser
                  </button>
                </div>
              </div>
            </Show>
          </div>
        </div>
      </Show>

      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.8rem", "white-space": "pre-wrap" }}>{err()}</p>
      </Show>
      <Show when={notice()}>
        <p style={{ color: "var(--success)", "font-size": "0.8rem", "white-space": "pre-wrap" }}>{notice()}</p>
      </Show>
      <Show when={conflicts().length > 0}>
        <div style={{ border: "1px solid var(--warning-border)", background: "var(--warning-bg)", padding: "0.4rem", "font-size": "0.8rem" }}>
          <div style={{ "font-weight": 700, "margin-bottom": "0.25rem" }}>Conflicted files</div>
          <ul style={{ margin: 0, padding: "0 0 0 1.2rem" }}>
            <For each={conflicts()}>{(path) => <li>{path}</li>}</For>
          </ul>
          <button style={{ ...smallBtn, "margin-top": "0.35rem" }} onClick={abortOperation}>
            Abort
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
                draggable={true}
                onDragStart={(ev) => ev.dataTransfer?.setData("text/plain", r.name)}
                onDragOver={(ev) => ev.preventDefault()}
                onDrop={(ev) => onBranchDrop(r.name, ev)}
                style={{
                  display: "flex",
                  "align-items": "center",
                  gap: "0.4rem",
                  "font-family": "monospace",
                  "font-size": "0.85rem",
                  padding: "0.1rem 0",
                }}
              >
                <span style={{ width: "0.8rem", color: "var(--success)" }}>{isCurrent(r) ? "●" : ""}</span>
                <span style={{ flex: "1", "font-weight": isCurrent(r) ? 700 : 400 }}>{r.name}</span>
                <span style={{ color: "var(--fg-muted)" }}>{r.target.slice(0, 8)}</span>
                <button
                  style={smallBtn}
                  title={`Create a ${noun()} from this branch`}
                  onClick={() => openPrForm(r.name)}
                >
                  PR
                </button>
                <Show when={isCurrent(r)}>
                  <button
                    style={smallBtn}
                    title="Interactive rebase this branch's recent history"
                    onClick={() => props.onInteractiveRebase(r.target)}
                  >
                    Rebase i
                  </button>
                </Show>
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
                <span style={{ color: "var(--fg-muted)" }}>{r.target.slice(0, 8)}</span>
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
                  <span style={{ color: "var(--fg-muted)" }}>{r.target.slice(0, 8)}</span>
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
