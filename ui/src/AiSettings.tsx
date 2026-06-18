import { createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import type { RepoId } from "./commands";
import {
  type AiConfig,
  type ProviderKind,
  PROVIDERS,
  PROVIDER_LABEL,
  deleteAiKey,
  getAiConfig,
  grantConsent,
  hasAiKey,
  isRemote,
  ollamaModels,
  repoEnabled,
  revokeConsent,
  setAiConfig,
  setAiKey,
  setRepoEnabled,
} from "./ai";

/**
 * AI settings (PH5-002): pick the active provider, store BYOK keys in the OS
 * keychain, grant/revoke per-provider remote-send consent, and toggle AI for
 * this repo (default off). Ollama is surfaced as the local-first option.
 */
const AiSettings: Component<{ repoId: RepoId }> = (props) => {
  const [cfg, setCfg] = createSignal<AiConfig | null>(null);
  const [keyInput, setKeyInput] = createSignal("");
  const [keyStored, setKeyStored] = createSignal(false);
  const [models, setModels] = createSignal<string[]>([]);
  const [enabled, setEnabled] = createSignal(false);
  const [notice, setNotice] = createSignal<string | null>(null);
  const [err, setErr] = createSignal<string | null>(null);

  const load = async () => {
    try {
      const c = await getAiConfig();
      setCfg(c);
      setEnabled(await repoEnabled(props.repoId));
      if (c.active) setKeyStored(await hasAiKey(c.active));
    } catch (e) {
      setErr(String(e));
    }
  };
  onMount(load);

  const active = () => cfg()?.active ?? null;

  const persist = async (next: AiConfig) => {
    setCfg(next);
    try {
      await setAiConfig(next);
      setNotice("Saved.");
      setTimeout(() => setNotice(null), 1500);
    } catch (e) {
      setErr(String(e));
    }
  };

  const pickProvider = async (p: ProviderKind) => {
    const c = cfg();
    if (!c) return;
    await persist({ ...c, active: p });
    setKeyStored(await hasAiKey(p));
    setKeyInput("");
  };

  const saveKey = async () => {
    const p = active();
    if (!p || !keyInput().trim()) return;
    try {
      await setAiKey(p, keyInput().trim());
      setKeyInput("");
      setKeyStored(true);
      setNotice("Key stored in the OS keychain.");
      setTimeout(() => setNotice(null), 1800);
    } catch (e) {
      setErr(String(e));
    }
  };

  const clearKey = async () => {
    const p = active();
    if (!p) return;
    await deleteAiKey(p).catch((e) => setErr(String(e)));
    setKeyStored(false);
  };

  const toggleConsent = async (p: ProviderKind, on: boolean) => {
    const c = cfg();
    if (!c) return;
    try {
      if (on) await grantConsent(p);
      else await revokeConsent(p);
      const consented = on
        ? [...c.consented, p]
        : c.consented.filter((x) => x !== p);
      setCfg({ ...c, consented });
    } catch (e) {
      setErr(String(e));
    }
  };

  const toggleRepo = async (on: boolean) => {
    setEnabled(on);
    await setRepoEnabled(props.repoId, on).catch((e) => setErr(String(e)));
  };

  const refreshModels = async () => {
    try {
      setModels(await ollamaModels());
      setNotice(`${models().length} local model(s).`);
      setTimeout(() => setNotice(null), 1800);
    } catch (e) {
      setErr(String(e));
    }
  };

  const field = {
    border: "1px solid var(--border)",
    "border-radius": "4px",
    padding: "0.25rem 0.4rem",
    "font-size": "0.82rem",
    background: "var(--surface)",
    color: "var(--fg)",
  } as const;

  return (
    <div>
      <h3 style={{ margin: "1.2rem 0 0.3rem", "font-size": "0.95rem" }}>AI</h3>
      <p style={{ "font-size": "0.78rem", color: "var(--fg-muted, #888)", margin: "0 0 0.6rem" }}>
        Bring your own keys. Remote providers receive code/diffs only after you
        consent; Ollama runs locally and never leaves your machine. Redaction is
        best-effort, not a guarantee.
      </p>

      <Show when={cfg()} fallback={<p style={{ "font-size": "0.82rem" }}>Loading…</p>}>
        {/* Per-repo toggle */}
        <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.85rem", "margin-bottom": "0.6rem" }}>
          <input type="checkbox" checked={enabled()} onChange={(e) => toggleRepo(e.currentTarget.checked)} />
          Enable AI for this repository
        </label>

        {/* Active provider */}
        <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "margin-bottom": "0.5rem" }}>
          <span style={{ width: "6rem", "font-size": "0.82rem" }}>Provider</span>
          <select
            style={field}
            value={active() ?? ""}
            onChange={(e) => pickProvider(e.currentTarget.value as ProviderKind)}
          >
            <option value="" disabled>
              Select…
            </option>
            <For each={PROVIDERS}>
              {(p) => <option value={p}>{PROVIDER_LABEL[p]}</option>}
            </For>
          </select>
        </div>

        {/* Default model */}
        <Show when={active()}>
          <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "margin-bottom": "0.5rem" }}>
            <span style={{ width: "6rem", "font-size": "0.82rem" }}>Model</span>
            <input
              style={{ ...field, flex: "1" }}
              placeholder="default model id"
              value={cfg()!.default_model ?? ""}
              onChange={(e) => persist({ ...cfg()!, default_model: e.currentTarget.value || null })}
            />
          </div>
        </Show>

        {/* Local Ollama config */}
        <Show when={active() === "Ollama"}>
          <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "margin-bottom": "0.5rem" }}>
            <span style={{ width: "6rem", "font-size": "0.82rem" }}>Ollama host</span>
            <input
              style={{ ...field, flex: "1" }}
              value={cfg()!.ollama_host}
              onChange={(e) => persist({ ...cfg()!, ollama_host: e.currentTarget.value })}
            />
            <button style={field} onClick={refreshModels}>
              List models
            </button>
          </div>
          <Show when={models().length > 0}>
            <div style={{ "font-size": "0.78rem", "margin-bottom": "0.5rem" }}>
              Local models:{" "}
              <For each={models()}>
                {(m) => (
                  <button
                    style={{ ...field, "margin-right": "0.3rem", cursor: "pointer" }}
                    onClick={() => persist({ ...cfg()!, default_model: m })}
                  >
                    {m}
                  </button>
                )}
              </For>
            </div>
          </Show>
        </Show>

        {/* Azure-specific config */}
        <Show when={active() === "AzureOpenAi"}>
          <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "margin-bottom": "0.5rem" }}>
            <span style={{ width: "6rem", "font-size": "0.82rem" }}>Endpoint</span>
            <input
              style={{ ...field, flex: "1" }}
              placeholder="https://my.openai.azure.com"
              value={cfg()!.azure_endpoint}
              onChange={(e) => persist({ ...cfg()!, azure_endpoint: e.currentTarget.value })}
            />
          </div>
          <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "margin-bottom": "0.5rem" }}>
            <span style={{ width: "6rem", "font-size": "0.82rem" }}>Deployment</span>
            <input
              style={{ ...field, flex: "1" }}
              value={cfg()!.azure_deployment}
              onChange={(e) => persist({ ...cfg()!, azure_deployment: e.currentTarget.value })}
            />
          </div>
        </Show>

        {/* API key (remote providers only) */}
        <Show when={active() && isRemote(active()!)}>
          <div style={{ display: "flex", "align-items": "center", gap: "0.4rem", "margin-bottom": "0.5rem" }}>
            <span style={{ width: "6rem", "font-size": "0.82rem" }}>API key</span>
            <input
              type="password"
              style={{ ...field, flex: "1" }}
              placeholder={keyStored() ? "•••••••• (stored)" : "paste key…"}
              value={keyInput()}
              onInput={(e) => setKeyInput(e.currentTarget.value)}
            />
            <button style={field} onClick={saveKey}>
              Save
            </button>
            <Show when={keyStored()}>
              <button style={field} onClick={clearKey}>
                Clear
              </button>
            </Show>
          </div>

          {/* Consent (ADR-0009) */}
          <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.82rem", "margin-bottom": "0.6rem" }}>
            <input
              type="checkbox"
              checked={cfg()!.consented.includes(active()!)}
              onChange={(e) => toggleConsent(active()!, e.currentTarget.checked)}
            />
            I consent to sending code/diffs to {PROVIDER_LABEL[active()!]} (data leaves this machine)
          </label>
        </Show>
      </Show>

      <h3 style={{ margin: "1.2rem 0 0.3rem", "font-size": "0.95rem" }}>MCP server</h3>
      <p style={{ "font-size": "0.78rem", color: "var(--fg-muted, #888)", margin: "0 0 0.6rem" }}>
        Expose this repo's context (status, diff, log, file-at-rev, blame,
        commit search) to an external assistant (Claude/Cursor) via the{" "}
        <code>lady-mcp</code> server. It is read-only — no mutating tools. Add an
        MCP entry that runs <code>lady-mcp &lt;repo-path&gt;</code>; remove it to
        disable.
      </p>

      <Show when={notice()}>
        <p style={{ color: "#1a7f37", "font-size": "0.8rem", margin: "0.2rem 0" }}>{notice()}</p>
      </Show>
      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.8rem", margin: "0.2rem 0" }}>{err()}</p>
      </Show>
    </div>
  );
};

export default AiSettings;
