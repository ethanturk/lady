import { createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { HostingInfo, LicenseStatus, RepoId } from "./commands";
import { FORGE_LABEL } from "./commands";

/**
 * Settings panel (PH3-011 / PH4): connect/disconnect the active repo's forge
 * (GitHub / GitLab / Bitbucket / Azure DevOps, auto-detected from the remote)
 * via a token stored in the OS keychain (never on disk), plus license entry.
 */
const SettingsView: Component<{ repoId: RepoId }> = (props) => {
  const [status, setStatus] = createSignal<HostingInfo>({
    kind: null,
    connected: false,
    login: null,
    slug: null,
  });
  const [token, setToken] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const forgeName = () => (status().kind ? FORGE_LABEL[status().kind!] : "forge");

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
    invoke<HostingInfo>("hosting_status", { repo: props.repoId })
      .then(setStatus)
      .catch((e) => setErr(String(e)));
  };

  onMount(() => {
    loadStatus();
    invoke<LicenseStatus>("license_status").then(setLicense).catch(() => {});
  });

  const connect = () => {
    if (!token().trim()) return;
    setBusy(true);
    setErr(null);
    invoke<HostingInfo>("hosting_connect", { repo: props.repoId, token: token().trim() })
      .then((s) => {
        setStatus(s);
        setToken("");
      })
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  };

  const disconnect = () => {
    setBusy(true);
    invoke("hosting_sign_out", { repo: props.repoId })
      .then(() => loadStatus())
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

      <h3 style={{ margin: "1.2rem 0 0.6rem", "font-size": "0.95rem" }}>Hosting — {forgeName()}</h3>

      <Show when={err()}>
        <p style={{ color: "crimson", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      <Show
        when={status().kind}
        fallback={<p style={{ color: "#888", "font-size": "0.82rem" }}>No supported forge remote detected.</p>}
      >
        <Show
          when={status().connected}
          fallback={
            <div>
              <p style={{ "font-size": "0.85rem", color: "#444" }}>
                Connect to {forgeName()} with a personal access token (stored in your OS keychain —
                never on disk or in logs).
              </p>
              <div style={{ display: "flex", gap: "0.4rem", "align-items": "center" }}>
                <input
                  type="password"
                  style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
                  placeholder="personal access token"
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
              ✓ Connected to {forgeName()}{status().login ? ` as ${status().login}` : ""}
            </span>
            <button onClick={disconnect} disabled={busy()} style={{ padding: "0.25rem 0.8rem" }}>
              Disconnect
            </button>
          </div>
        </Show>

        <h4 style={{ margin: "1rem 0 0.3rem", "font-size": "0.85rem" }}>Detected repository</h4>
        <Show
          when={status().slug}
          fallback={<p style={{ color: "#888", "font-size": "0.82rem" }}>Could not parse a repo from the remote.</p>}
        >
          <p style={{ "font-family": "monospace", "font-size": "0.85rem" }}>
            {status().slug!.project
              ? `${status().slug!.owner}/${status().slug!.project}/${status().slug!.repo}`
              : `${status().slug!.owner}/${status().slug!.repo}`}
          </p>
        </Show>
      </Show>
    </div>
  );
};

export default SettingsView;
