import { createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { GithubStatus, LicenseStatus, RepoSlug, RepoId } from "./commands";

/**
 * Settings panel (PH3-011): GitHub connect/disconnect via a personal access
 * token (stored in the OS keychain, never on disk), plus the detected GitHub
 * repo for the active repository.
 */
const SettingsView: Component<{ repoId: RepoId }> = (props) => {
  const [status, setStatus] = createSignal<GithubStatus>({ connected: false, login: null });
  const [token, setToken] = createSignal("");
  const [slug, setSlug] = createSignal<RepoSlug | null>(null);
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  // Licensing (PH3-013).
  const [license, setLicense] = createSignal<LicenseStatus | null>(null);
  const [licenseKey, setLicenseKey] = createSignal("");
  const [licenseErr, setLicenseErr] = createSignal<string | null>(null);

  const describeLicense = (s: LicenseStatus | null) => {
    if (!s) return "…";
    if (s.kind === "Licensed") return `Licensed to ${s.licensee}`;
    if (s.kind === "Trial") return `Trial — ${s.days_left} day(s) left`;
    return "Trial expired";
  };

  const activateLicense = () => {
    if (!licenseKey().trim()) return;
    setLicenseErr(null);
    invoke<LicenseStatus>("license_activate", { key: licenseKey().trim() })
      .then((s) => {
        setLicense(s);
        setLicenseKey("");
      })
      .catch((e) => setLicenseErr(String(e)));
  };

  const loadStatus = () => {
    invoke<GithubStatus>("github_auth_status")
      .then(setStatus)
      .catch((e) => setErr(String(e)));
  };

  onMount(() => {
    loadStatus();
    invoke<RepoSlug | null>("github_detect", { repo: props.repoId })
      .then(setSlug)
      .catch(() => setSlug(null));
    invoke<LicenseStatus>("license_status").then(setLicense).catch(() => {});
  });

  const connect = () => {
    if (!token().trim()) return;
    setBusy(true);
    setErr(null);
    invoke<GithubStatus>("github_auth_start", { token: token().trim() })
      .then((s) => {
        setStatus(s);
        setToken("");
      })
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  };

  const disconnect = () => {
    setBusy(true);
    invoke("github_sign_out")
      .then(() => setStatus({ connected: false, login: null }))
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.9rem 1rem", "max-width": "40rem" }}>
      <h3 style={{ margin: "0 0 0.4rem", "font-size": "0.95rem" }}>License</h3>
      <p style={{ "font-size": "0.85rem", color: "#444", margin: "0 0 0.4rem" }}>{describeLicense(license())}</p>
      <div style={{ display: "flex", gap: "0.4rem", "align-items": "center" }}>
        <input
          style={{ flex: "1", padding: "0.3rem 0.5rem", "font-family": "monospace", "font-size": "0.8rem" }}
          placeholder="license key"
          value={licenseKey()}
          onInput={(e) => setLicenseKey(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Enter" && activateLicense()}
        />
        <button onClick={activateLicense} style={{ padding: "0.3rem 0.9rem" }}>
          Activate
        </button>
      </div>
      <Show when={licenseErr()}>
        <p style={{ color: "crimson", "font-size": "0.82rem" }}>{licenseErr()}</p>
      </Show>

      <h3 style={{ margin: "1.2rem 0 0.6rem", "font-size": "0.95rem" }}>GitHub</h3>

      <Show when={err()}>
        <p style={{ color: "crimson", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      <Show
        when={status().connected}
        fallback={
          <div>
            <p style={{ "font-size": "0.85rem", color: "#444" }}>
              Connect with a personal access token (stored in your OS keychain — never on disk or
              in logs).
            </p>
            <div style={{ display: "flex", gap: "0.4rem", "align-items": "center" }}>
              <input
                type="password"
                style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
                placeholder="ghp_… personal access token"
                value={token()}
                onInput={(e) => setToken(e.currentTarget.value)}
                onKeyDown={(e) => e.key === "Enter" && connect()}
              />
              <button onClick={connect} disabled={busy()} style={{ padding: "0.3rem 0.9rem" }}>
                {busy() ? "Connecting…" : "Connect"}
              </button>
            </div>
          </div>
        }
      >
        <div style={{ display: "flex", "align-items": "center", gap: "0.6rem" }}>
          <span style={{ color: "#1a7f37", "font-size": "0.9rem" }}>
            ✓ Connected{status().login ? ` as ${status().login}` : ""}
          </span>
          <button onClick={disconnect} disabled={busy()} style={{ padding: "0.25rem 0.8rem" }}>
            Disconnect
          </button>
        </div>
      </Show>

      <h4 style={{ margin: "1rem 0 0.3rem", "font-size": "0.85rem" }}>Detected repository</h4>
      <Show
        when={slug()}
        fallback={<p style={{ color: "#888", "font-size": "0.82rem" }}>No GitHub remote detected.</p>}
      >
        <p style={{ "font-family": "monospace", "font-size": "0.85rem" }}>
          {slug()!.owner}/{slug()!.repo}
        </p>
      </Show>
    </div>
  );
};

export default SettingsView;
