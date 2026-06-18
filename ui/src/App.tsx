import { createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, RefInfo, RefKind, RepoId } from "./commands";

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

const App: Component = () => {
  const [info, setInfo] = createSignal<AppInfo | null>(null);
  const [path, setPath] = createSignal("");
  const [refs, setRefs] = createSignal<RefInfo[]>([]);
  const [opened, setOpened] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);

  onMount(async () => {
    const data = await invoke<AppInfo>("app_info");
    setInfo(data);
  });

  const openRepo = async () => {
    try {
      setErr(null);
      const id = await invoke<RepoId>("open_repo", { path: path() });
      const refList = await invoke<RefInfo[]>("list_refs", { repo: id });
      setRefs(refList);
      setOpened(true);
    } catch (e) {
      setErr(String(e));
      setOpened(false);
    }
  };

  const byKind = (kind: RefKind) => refs().filter((r) => r.kind === kind);

  return (
    <div style={{ padding: "1rem", "font-family": "sans-serif" }}>
      <Show when={info()}>
        <h2 style={{ margin: "0 0 1rem" }}>
          {info()?.name} {info()?.version}
        </h2>
      </Show>

      <div
        style={{ display: "flex", gap: "0.5rem", "margin-bottom": "0.75rem" }}
      >
        <input
          type="text"
          value={path()}
          onInput={(e) => setPath(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") openRepo();
          }}
          placeholder="/path/to/repo"
          style={{ flex: "1", padding: "0.25rem 0.5rem" }}
        />
        <button onClick={openRepo}>Open</button>
      </div>

      <Show when={err()}>
        <p style={{ color: "crimson", margin: "0.25rem 0" }}>{err()}</p>
      </Show>

      <Show when={opened()}>
        <div>
          <RefGroup title="HEAD" refs={byKind("Head")} />
          <RefGroup title="Branches" refs={byKind("Branch")} />
          <RefGroup title="Tags" refs={byKind("Tag")} />
          <RefGroup title="Remotes" refs={byKind("Remote")} />
        </div>
      </Show>
    </div>
  );
};

export default App;
