import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { FlowConfig, FlowKind, RepoId } from "./commands";

const KINDS: FlowKind[] = ["Feature", "Release", "Hotfix"];

/**
 * git-flow panel (PH4-008): initialize git-flow, then start/finish feature,
 * release, and hotfix branches. Finishing a release/hotfix tags + merges into
 * master and develop per the persisted config. Native semantics (no git-flow
 * binary required).
 */
const GitFlowView: Component<{
  repoId: RepoId;
  refreshNonce: number;
  onChanged: () => void;
}> = (props) => {
  const [config, setConfig] = createSignal<FlowConfig | null>(null);
  const [err, setErr] = createSignal<string | null>(null);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [kind, setKind] = createSignal<FlowKind>("Feature");
  const [name, setName] = createSignal("");
  const [master, setMaster] = createSignal("main");

  const reload = () => {
    invoke<FlowConfig>("flow_config", { repo: props.repoId })
      .then((c) => {
        setConfig(c);
        setMaster(c.master);
      })
      .catch((e) => setErr(String(e)));
  };
  createEffect(() => {
    props.refreshNonce;
    props.repoId;
    reload();
  });

  const init = () => {
    setErr(null);
    const base = config()!;
    invoke("flow_init", { repo: props.repoId, config: { ...base, master: master() } })
      .then(() => {
        setNotice("git-flow initialized.");
        reload();
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const start = () => {
    if (!name().trim()) return;
    setErr(null);
    setNotice(null);
    invoke<string>("flow_start", { repo: props.repoId, kind: kind(), name: name().trim() })
      .then((branch) => {
        setNotice(`Started ${branch}.`);
        setName("");
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  const finish = () => {
    if (!name().trim()) return;
    setErr(null);
    setNotice(null);
    invoke("flow_finish", { repo: props.repoId, kind: kind(), name: name().trim() })
      .then(() => {
        setNotice(`Finished ${kind().toLowerCase()} ${name().trim()}.`);
        setName("");
        props.onChanged();
      })
      .catch((e) => setErr(String(e)));
  };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.85rem 1rem", "max-width": "34rem" }}>
      <h3 style={{ margin: "0 0 0.5rem", "font-size": "0.95rem" }}>git-flow</h3>

      <Show when={err()}>
        <p style={{ color: "crimson", "font-size": "0.85rem", "white-space": "pre-wrap" }}>{err()}</p>
      </Show>
      <Show when={notice()}>
        <p style={{ color: "#1a7f37", "font-size": "0.85rem" }}>{notice()}</p>
      </Show>

      <Show
        when={config()?.initialized}
        fallback={
          <div style={{ display: "flex", gap: "0.4rem", "align-items": "center" }}>
            <label style={{ "font-size": "0.82rem" }}>
              production branch{" "}
              <input style={{ "font-size": "0.82rem", width: "8rem" }} value={master()} onInput={(e) => setMaster(e.currentTarget.value)} />
            </label>
            <button onClick={init} style={{ padding: "0.3rem 0.9rem" }}>
              Initialize git-flow
            </button>
          </div>
        }
      >
        <p style={{ "font-size": "0.82rem", color: "#666" }}>
          master <b>{config()!.master}</b> · develop <b>{config()!.develop}</b> · prefixes{" "}
          <span style={{ "font-family": "monospace" }}>
            {config()!.feature_prefix} {config()!.release_prefix} {config()!.hotfix_prefix}
          </span>
        </p>

        <div style={{ display: "flex", gap: "0.4rem", "align-items": "center", "margin-top": "0.5rem" }}>
          <select value={kind()} onChange={(e) => setKind(e.currentTarget.value as FlowKind)} style={{ "font-size": "0.82rem" }}>
            <For each={KINDS}>{(k) => <option value={k}>{k}</option>}</For>
          </select>
          <input
            style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
            placeholder={kind() === "Feature" ? "feature name" : "version"}
            value={name()}
            onInput={(e) => setName(e.currentTarget.value)}
          />
          <button onClick={start} style={{ padding: "0.3rem 0.8rem" }}>Start</button>
          <button onClick={finish} style={{ padding: "0.3rem 0.8rem" }}>Finish</button>
        </div>
        <p style={{ color: "#888", "font-size": "0.72rem" }}>
          Finish merges Feature → develop; Release/Hotfix → master (tagged) + develop. A merge
          conflict pauses for resolution in the Conflicts tab.
        </p>
      </Show>
    </div>
  );
};

export default GitFlowView;
