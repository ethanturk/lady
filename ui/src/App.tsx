import { createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo } from "./commands";

const App: Component = () => {
  const [info, setInfo] = createSignal<AppInfo | null>(null);

  onMount(async () => {
    const data = await invoke<AppInfo>("app_info");
    setInfo(data);
  });

  return (
    <div style={{ padding: "2rem", "font-family": "sans-serif" }}>
      <Show when={info()} fallback={<p>Loading…</p>}>
        <h1>{info()?.name}</h1>
        <p>v{info()?.version}</p>
      </Show>
    </div>
  );
};

export default App;
