import { createSignal } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { LicenseStatus } from "./commands";

/**
 * Full-screen licensing gate (PH3-013) shown when the trial has expired and no
 * valid license is active. Blocks the main UI until a valid Ed25519-signed key
 * is entered. This is a feature gate, not a security boundary (ADR-0007).
 */
const LicenseGate: Component<{ onActivated: (status: LicenseStatus) => void }> = (props) => {
  const [key, setKey] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const activate = () => {
    if (!key().trim()) return;
    setBusy(true);
    setErr(null);
    invoke<LicenseStatus>("license_activate", { key: key().trim() })
      .then((status) => props.onActivated(status))
      .catch((e) => setErr(String(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: "0",
        background: "rgba(20,22,28,0.92)",
        display: "flex",
        "align-items": "center",
        "justify-content": "center",
        "z-index": "100",
        "font-family": "sans-serif",
      }}
    >
      <div style={{ background: "var(--surface)", "border-radius": "8px", padding: "1.5rem", width: "460px", "max-width": "92vw" }}>
        <h2 style={{ margin: "0 0 0.5rem", "font-size": "1.1rem" }}>Trial expired</h2>
        <p style={{ "font-size": "0.88rem", color: "var(--fg)" }}>
          Your 30-day trial has ended. Enter a license key to continue using Lady.
        </p>
        <input
          type="text"
          style={{ width: "100%", "box-sizing": "border-box", padding: "0.4rem 0.5rem", "font-family": "monospace", "font-size": "0.8rem" }}
          placeholder="license key"
          value={key()}
          onInput={(e) => setKey(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Enter" && activate()}
        />
        {err() && <p style={{ color: "var(--error)", "font-size": "0.82rem" }}>{err()}</p>}
        <button
          onClick={activate}
          disabled={busy()}
          style={{ "margin-top": "0.6rem", background: "var(--accent)", color: "var(--on-accent)", border: "none", "border-radius": "4px", padding: "0.4rem 1rem", cursor: "pointer" }}
        >
          {busy() ? "Activating…" : "Activate"}
        </button>
      </div>
    </div>
  );
};

export default LicenseGate;
