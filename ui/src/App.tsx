import { createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, RefInfo, RefKind, RepoId } from "./commands";
import GraphView from "./GraphView";
import DiffView from "./DiffView";
import BlameView from "./BlameView";
import FileHistory from "./FileHistory";

interface RefGroupProps {
  title: string;
  refs: RefInfo[];
}

const RefGroup: Component<RefGroupProps> = (props) => (
  <Show when={props.refs.length > 0}>
    <section>
      <h3 style={{ margin: "0.5rem 0 0.25rem" }}>{props.title}</h3>
      <ul style={{ margin: 0, "padding-left": "1.2rem" }}>
        <For each={props.refs}>
          {(ref) => (
            <li style={{ "font-family": "monospace", "font-size": "0.85rem" }}>
              {ref.name}
              <span style={{ color: "#888", "margin-left": "0.5rem" }}>
                {ref.target.slice(0, 8)}
              </span>
            </li>
          )}
        </For>
      </ul>
    </section>
  </Show>
);

type Tab = "commits" | "refs" | "blame" | "history";

const App: Component = () => {
  const [info, setInfo] = createSignal<AppInfo | null>(null);
  const [path, setPath] = createSignal("");
  const [repoId, setRepoId] = createSignal<RepoId | null>(null);
  const [refs, setRefs] = createSignal<RefInfo[]>([]);
  const [tab, setTab] = createSignal<Tab>("commits");
  const [selectedCommit, setSelectedCommit] = createSignal<string | null>(null);
  const [err, setErr] = createSignal<string | null>(null);

  onMount(async () => {
    const data = await invoke<AppInfo>("app_info");
    setInfo(data);
  });

  const openRepo = async () => {
    try {
      setErr(null);
      setRepoId(null);
      setRefs([]);
      setSelectedCommit(null);
      const id = await invoke<RepoId>("open_repo", { path: path() });
      const refList = await invoke<RefInfo[]>("list_refs", { repo: id });
      setRefs(refList);
      setRepoId(id);
      setTab("commits");
    } catch (e) {
      setErr(String(e));
    }
  };

  const byKind = (kind: RefKind) => refs().filter((r) => r.kind === kind);

  const tabStyle = (t: Tab) => ({
    padding: "0.3rem 0.9rem",
    cursor: "pointer",
    border: "none",
    background: tab() === t ? "#0070f3" : "#eee",
    color: tab() === t ? "#fff" : "#333",
    "border-radius": "4px 4px 0 0",
    "font-size": "0.875rem",
  });

  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        "flex-direction": "column",
        "font-family": "sans-serif",
      }}
    >
      {/* Header */}
      <div style={{ padding: "0.75rem 1rem", "flex-shrink": 0 }}>
        <Show when={info()}>
          <span style={{ "font-weight": 600 }}>
            {info()?.name} {info()?.version}
          </span>
        </Show>

        <div
          style={{
            display: "flex",
            gap: "0.5rem",
            "margin-top": "0.5rem",
          }}
        >
          <input
            type="text"
            value={path()}
            onInput={(e) => setPath(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") openRepo();
            }}
            placeholder="/path/to/repo"
            style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.875rem" }}
          />
          <button onClick={openRepo} style={{ padding: "0.3rem 0.8rem" }}>
            Open
          </button>
        </div>

        <Show when={err()}>
          <p style={{ color: "crimson", margin: "0.25rem 0 0", "font-size": "0.85rem" }}>
            {err()}
          </p>
        </Show>

        <Show when={repoId()}>
          <div style={{ display: "flex", gap: "0.25rem", "margin-top": "0.5rem" }}>
            <button style={tabStyle("commits")} onClick={() => setTab("commits")}>
              Commits
            </button>
            <button style={tabStyle("refs")} onClick={() => setTab("refs")}>
              Refs
            </button>
            <button style={tabStyle("blame")} onClick={() => setTab("blame")}>
              Blame
            </button>
            <button style={tabStyle("history")} onClick={() => setTab("history")}>
              History
            </button>
          </div>
        </Show>
      </div>

      {/* Content */}
      <Show when={repoId()}>
        <div style={{ flex: "1", overflow: "hidden" }}>
          <Show when={tab() === "commits"}>
            <div style={{ display: "flex", height: "100%", overflow: "hidden" }}>
              <div style={{ flex: "1", "min-width": "0", overflow: "hidden" }}>
                <GraphView
                  repoId={repoId()!}
                  selected={selectedCommit() ?? undefined}
                  onSelectCommit={setSelectedCommit}
                />
              </div>
              <Show when={selectedCommit()}>
                <div
                  style={{
                    flex: "1",
                    "min-width": "0",
                    "border-left": "1px solid #ddd",
                    overflow: "hidden",
                  }}
                >
                  <DiffView repoId={repoId()!} commit={selectedCommit()!} />
                </div>
              </Show>
            </div>
          </Show>
          <Show when={tab() === "refs"}>
            <div style={{ padding: "0.5rem 1rem", "overflow-y": "auto", height: "100%" }}>
              <RefGroup title="HEAD" refs={byKind("Head")} />
              <RefGroup title="Branches" refs={byKind("Branch")} />
              <RefGroup title="Tags" refs={byKind("Tag")} />
              <RefGroup title="Remotes" refs={byKind("Remote")} />
            </div>
          </Show>
          <Show when={tab() === "blame"}>
            <BlameView repoId={repoId()!} />
          </Show>
          <Show when={tab() === "history"}>
            <FileHistory repoId={repoId()!} />
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default App;
