import { createEffect, createSignal, For, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type {
  CommandOutput,
  CustomCommand,
  Placeholder,
  RefInfo,
  RepoId,
  Settings,
} from "./commands";

/**
 * Custom commands (PH3-009): a builder to create/edit settings-persisted
 * commands ({name, template}) and a runner that prompts for the template's
 * typed placeholders, runs it safely (argv, not shell), and shows the output.
 */
const CustomCommandsView: Component<{
  repoId: RepoId;
  refs: RefInfo[];
  files: string[];
}> = (props) => {
  const [commands, setCommands] = createSignal<CustomCommand[]>([]);
  const [recent, setRecent] = createSignal<Settings["recent"]>([]);
  const [name, setName] = createSignal("");
  const [template, setTemplate] = createSignal("");
  const [editing, setEditing] = createSignal<number | null>(null);
  const [err, setErr] = createSignal<string | null>(null);

  // Runner state.
  const [running, setRunning] = createSignal<CustomCommand | null>(null);
  const [placeholders, setPlaceholders] = createSignal<Placeholder[]>([]);
  const [values, setValues] = createSignal<Record<string, string>>({});
  const [output, setOutput] = createSignal<CommandOutput | null>(null);
  const [busy, setBusy] = createSignal(false);

  createEffect(() => {
    invoke<Settings>("load_settings")
      .then((s) => {
        setCommands(s.custom_commands ?? []);
        setRecent(s.recent);
      })
      .catch((e) => setErr(String(e)));
  });

  const persist = (next: CustomCommand[]) => {
    setCommands(next);
    invoke("save_settings", { settings: { recent: recent(), custom_commands: next } }).catch((e) =>
      setErr(String(e)),
    );
  };

  const save = () => {
    if (!name().trim() || !template().trim()) return;
    const entry: CustomCommand = { name: name().trim(), template: template().trim() };
    const idx = editing();
    const next = idx == null ? [...commands(), entry] : commands().map((c, i) => (i === idx ? entry : c));
    persist(next);
    setName("");
    setTemplate("");
    setEditing(null);
  };

  const edit = (i: number) => {
    const c = commands()[i];
    setName(c.name);
    setTemplate(c.template);
    setEditing(i);
  };

  const remove = (i: number) => persist(commands().filter((_, j) => j !== i));

  // Open the runner: parse the template's placeholders to prompt for.
  const openRunner = (c: CustomCommand) => {
    setOutput(null);
    setValues({});
    setRunning(c);
    invoke<Placeholder[]>("parse_placeholders", { template: c.template })
      .then(setPlaceholders)
      .catch((e) => setErr(String(e)));
  };

  const run = () => {
    const c = running();
    if (!c) return;
    setBusy(true);
    setErr(null);
    invoke<CommandOutput>("run_custom_command", {
      repo: props.repoId,
      template: c.template,
      values: values(),
    })
      .then(setOutput)
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  };

  const setVal = (k: string, v: string) => setValues((prev) => ({ ...prev, [k]: v }));

  const branches = () => props.refs.filter((r) => r.kind === "Branch" || r.kind === "Remote").map((r) => r.name);

  const smallBtn = {
    border: "1px solid var(--border)",
    background: "var(--surface)",
    "border-radius": "3px",
    "font-size": "0.72rem",
    padding: "0 0.45rem",
    cursor: "pointer",
  };
  const input = { padding: "0.3rem 0.5rem", "font-size": "0.85rem" };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.85rem 1rem" }}>
      <h3 style={{ margin: "0 0 0.5rem", "font-size": "0.95rem" }}>Custom commands</h3>

      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      {/* Builder */}
      <div style={{ display: "flex", gap: "0.4rem", "flex-wrap": "wrap", "align-items": "center", "margin-bottom": "0.5rem" }}>
        <input style={{ ...input, width: "10rem" }} placeholder="name" value={name()} onInput={(e) => setName(e.currentTarget.value)} />
        <input
          style={{ ...input, flex: "1", "min-width": "18rem", "font-family": "monospace" }}
          placeholder="git log {rev:text} {branch:branch} -- {path:file}"
          value={template()}
          onInput={(e) => setTemplate(e.currentTarget.value)}
        />
        <button onClick={save} style={{ padding: "0.3rem 0.8rem" }}>
          {editing() == null ? "Add" : "Save"}
        </button>
      </div>
      <p style={{ color: "var(--fg-muted)", "font-size": "0.72rem", "margin-top": 0 }}>
        Placeholders: <code>{"{name:text}"}</code>, <code>{"{name:branch}"}</code>, <code>{"{name:file}"}</code>. Values are
        passed as separate arguments (never a shell string).
      </p>

      {/* Saved commands */}
      <ul style={{ margin: "0.4rem 0", padding: 0, "list-style": "none" }}>
        <For each={commands()}>
          {(c, i) => (
            <li style={{ display: "flex", "align-items": "center", gap: "0.5rem", padding: "0.3rem 0", "border-bottom": "1px solid var(--border)" }}>
              <span style={{ "font-weight": 600, "min-width": "8rem", "font-size": "0.85rem" }}>{c.name}</span>
              <span style={{ flex: "1", "font-family": "monospace", "font-size": "0.78rem", color: "var(--fg-muted)", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }} title={c.template}>
                {c.template}
              </span>
              <button style={smallBtn} onClick={() => openRunner(c)}>Run</button>
              <button style={smallBtn} onClick={() => edit(i())}>Edit</button>
              <button style={smallBtn} onClick={() => remove(i())}>Delete</button>
            </li>
          )}
        </For>
      </ul>

      {/* Runner */}
      <Show when={running()}>
        <div style={{ border: "1px solid var(--border)", "border-radius": "4px", padding: "0.6rem", "margin-top": "0.6rem" }}>
          <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "margin-bottom": "0.4rem" }}>
            <strong style={{ "font-size": "0.88rem" }}>Run: {running()!.name}</strong>
            <span style={{ flex: "1" }} />
            <button style={smallBtn} onClick={() => setRunning(null)}>Close</button>
          </div>

          <For each={placeholders()}>
            {(ph) => (
              <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "margin-bottom": "0.3rem" }}>
                <label style={{ "min-width": "8rem", "font-size": "0.82rem" }}>
                  {ph.name} <span style={{ color: "var(--fg-muted)" }}>({ph.kind.toLowerCase()})</span>
                </label>
                <Show when={ph.kind === "Branch"} fallback={
                  <Show when={ph.kind === "File"} fallback={
                    <input style={{ ...input, flex: "1" }} value={values()[ph.name] ?? ""} onInput={(e) => setVal(ph.name, e.currentTarget.value)} />
                  }>
                    <select style={{ ...input, flex: "1" }} onChange={(e) => setVal(ph.name, e.currentTarget.value)}>
                      <option value="">— file —</option>
                      <For each={props.files}>{(f) => <option value={f}>{f}</option>}</For>
                    </select>
                  </Show>
                }>
                  <select style={{ ...input, flex: "1" }} onChange={(e) => setVal(ph.name, e.currentTarget.value)}>
                    <option value="">— branch —</option>
                    <For each={branches()}>{(b) => <option value={b}>{b}</option>}</For>
                  </select>
                </Show>
              </div>
            )}
          </For>

          <button onClick={run} disabled={busy()} style={{ padding: "0.3rem 0.9rem", "margin-top": "0.3rem" }}>
            {busy() ? "Running…" : "Run"}
          </button>

          <Show when={output()}>
            <div style={{ "margin-top": "0.5rem" }}>
              <div style={{ "font-size": "0.78rem", color: output()!.exit_code === 0 ? "#1a7f37" : "#cf222e" }}>
                exit code: {output()!.exit_code}
              </div>
              <Show when={output()!.stdout}>
                <pre style={{ background: "var(--surface-2)", padding: "0.5rem", "font-size": "0.76rem", "max-height": "16rem", overflow: "auto", "white-space": "pre-wrap" }}>
                  {output()!.stdout}
                </pre>
              </Show>
              <Show when={output()!.stderr}>
                <pre style={{ background: "#fff5f5", color: "#86181d", padding: "0.5rem", "font-size": "0.76rem", "max-height": "10rem", overflow: "auto", "white-space": "pre-wrap" }}>
                  {output()!.stderr}
                </pre>
              </Show>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
};

export default CustomCommandsView;
