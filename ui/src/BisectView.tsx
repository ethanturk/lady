import { createEffect, createSignal, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { BisectState, RepoId } from "./commands";

const EMPTY: BisectState = { current_oid: null, remaining_steps_estimate: 0, suspected: null };

/**
 * Bisect panel (PH3-008): start a bisect from a bad + good ref, mark each
 * tested commit good/bad/skip, watch the remaining-steps estimate, and see the
 * suspected first-bad commit when it converges. Reset exits bisect.
 */
const BisectView: Component<{
  repoId: RepoId;
  refreshNonce: number;
  onChanged: () => void;
}> = (props) => {
  const [state, setState] = createSignal<BisectState>(EMPTY);
  const [bad, setBad] = createSignal("HEAD");
  const [good, setGood] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);

  // Active once we have a current commit or a verdict.
  const active = () => state().current_oid != null || state().suspected != null;

  const load = () => {
    invoke<BisectState>("bisect_state", { repo: props.repoId })
      .then(setState)
      .catch((e) => setErr(String(e)));
  };
  createEffect(() => {
    props.refreshNonce;
    props.repoId;
    load();
  });

  const start = () => {
    if (!bad() || !good()) return;
    setErr(null);
    invoke<BisectState>("bisect_start", { repo: props.repoId, bad: bad(), good: good() })
      .then((s) => {
        setState(s);
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const mark = (m: "good" | "bad" | "skip") => {
    setErr(null);
    invoke<BisectState>("bisect_mark", { repo: props.repoId, mark: m })
      .then((s) => {
        setState(s);
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const reset = () => {
    setErr(null);
    invoke("bisect_reset", { repo: props.repoId })
      .then(() => {
        setState(EMPTY);
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const markBtn = (m: "good" | "bad" | "skip", bg: string) => (
    <button
      onClick={() => mark(m)}
      disabled={state().suspected != null}
      style={{
        background: bg,
        color: "#fff",
        border: "none",
        "border-radius": "4px",
        padding: "0.35rem 1rem",
        cursor: "pointer",
        "font-size": "0.85rem",
      }}
    >
      {m}
    </button>
  );

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.9rem 1rem" }}>
      <h3 style={{ margin: "0 0 0.6rem", "font-size": "0.95rem" }}>Bisect</h3>

      <Show when={err()}>
        <p style={{ color: "crimson", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      <Show
        when={active()}
        fallback={
          <div style={{ display: "flex", gap: "0.4rem", "align-items": "center", "flex-wrap": "wrap" }}>
            <label style={{ "font-size": "0.82rem" }}>
              bad{" "}
              <input style={{ "font-size": "0.8rem", width: "9rem" }} value={bad()} onInput={(e) => setBad(e.currentTarget.value)} />
            </label>
            <label style={{ "font-size": "0.82rem" }}>
              good{" "}
              <input style={{ "font-size": "0.8rem", width: "9rem" }} placeholder="good ref/oid" value={good()} onInput={(e) => setGood(e.currentTarget.value)} />
            </label>
            <button onClick={start} style={{ padding: "0.3rem 0.9rem" }}>
              Start bisect
            </button>
          </div>
        }
      >
        <Show
          when={state().suspected}
          fallback={
            <div>
              <p style={{ "font-size": "0.88rem" }}>
                Testing commit{" "}
                <span style={{ "font-family": "monospace", "font-weight": 600 }}>
                  {state().current_oid?.slice(0, 10)}
                </span>
                <span style={{ color: "#888", "margin-left": "0.6rem" }}>
                  ~{state().remaining_steps_estimate} step
                  {state().remaining_steps_estimate === 1 ? "" : "s"} left
                </span>
              </p>
              <p style={{ "font-size": "0.82rem", color: "#666" }}>
                Test the checked-out commit, then mark it:
              </p>
              <div style={{ display: "flex", gap: "0.5rem" }}>
                {markBtn("good", "#1a7f37")}
                {markBtn("bad", "#cf222e")}
                {markBtn("skip", "#6e7781")}
              </div>
            </div>
          }
        >
          <div
            style={{
              background: "#fff8e5",
              border: "1px solid #f0c36d",
              "border-radius": "4px",
              padding: "0.6rem",
            }}
          >
            <div style={{ "font-weight": 700, color: "#bc4c00" }}>First bad commit found</div>
            <div style={{ "font-family": "monospace", "margin-top": "0.25rem" }}>
              {state().suspected}
            </div>
          </div>
        </Show>

        <button onClick={reset} style={{ "margin-top": "0.8rem", padding: "0.3rem 0.9rem" }}>
          Reset (exit bisect)
        </button>
      </Show>
    </div>
  );
};

export default BisectView;
