import { createEffect, createSignal, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { For } from "solid-js";
import type {
  FfMode,
  ForgeKind,
  GitHubAccount,
  GitIdentity,
  HostingInfo,
  LicenseStatus,
  RepoAuth,
  RepoId,
  RepoInfo,
  RepoSettings,
  ResolvedRepoSettings,
} from "./commands";
import { FORGE_KINDS, FORGE_LABEL } from "./commands";
import {
  addGitHubAccount,
  assignRepoAccount,
  listGitHubAccounts,
  removeGitHubAccount,
} from "./accounts";
import AiSettings from "./AiSettings";
import {
  accent,
  ACCENT_LABEL,
  ACCENTS,
  changesLayout,
  setAccent,
  SIZE_LABEL,
  SIZE_OPTIONS,
  setChangesLayout,
  setTextSize,
  setThemeMode,
  setUiPadding,
  textSize,
  THEME_LABEL,
  THEME_MODES,
  themeMode,
  uiPadding,
  wrapDiff,
  setWrapDiff,
} from "./prefs";
import type { Accent, SizeStep, ThemeMode } from "./prefs";
import {
  BUILTIN_FF,
  BUILTIN_SIGN,
  globalDefaults,
  repoIdentityGet,
  repoIdentitySet,
  repoSettings,
  setGlobalDefaults,
  setRepoOverride,
} from "./repoSettings";

/**
 * Settings panel (PH3-011 / PH4): connect/disconnect the active repo's forge
 * (GitHub / GitLab / Bitbucket / Azure DevOps, auto-detected from the remote)
 * via a token stored in the OS keychain (never on disk), plus license entry.
 */
const SettingsView: Component<{ repoId: RepoId | null }> = (props) => {
  const [status, setStatus] = createSignal<HostingInfo>({
    kind: null,
    connected: false,
    login: null,
    slug: null,
  });
  const [token, setToken] = createSignal("");
  const [err, setErr] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  // Auto-update (PH6-008) — explicit user action only, never silent.
  type UpdateInfo = { available: boolean; version: string | null; notes: string | null; current: string };
  const [update, setUpdate] = createSignal<UpdateInfo | null>(null);
  const [updateErr, setUpdateErr] = createSignal<string | null>(null);
  const [updBusy, setUpdBusy] = createSignal(false);

  const checkUpdates = () => {
    setUpdBusy(true);
    setUpdateErr(null);
    setUpdate(null);
    invoke<UpdateInfo>("check_for_updates")
      .then(setUpdate)
      .catch((e) => setUpdateErr(String(e)))
      .finally(() => setUpdBusy(false));
  };

  const installUpdate = () => {
    if (!confirm("Download and install the update now? Lady will restart.")) return;
    setUpdBusy(true);
    setUpdateErr(null);
    // Resolves into the relaunch; on success the app restarts, so we mainly
    // surface errors here.
    invoke("install_update")
      .catch((e) => {
        setUpdateErr(String(e));
        setUpdBusy(false);
      });
  };

  const forgeName = () => (status().kind ? FORGE_LABEL[status().kind!] : "forge");

  // Create remote repository (PH4-005).
  const [crForge, setCrForge] = createSignal<ForgeKind>("GitHub");
  const [crName, setCrName] = createSignal("");
  const [crOwner, setCrOwner] = createSignal("");
  const [crProject, setCrProject] = createSignal("");
  const [crPrivate, setCrPrivate] = createSignal(true);
  const [crDesc, setCrDesc] = createSignal("");
  const [crOrigin, setCrOrigin] = createSignal(true);
  const [crResult, setCrResult] = createSignal<RepoInfo | null>(null);
  const [crErr, setCrErr] = createSignal<string | null>(null);
  const [crBusy, setCrBusy] = createSignal(false);

  // Bitbucket needs a workspace; Azure needs org (owner) + project.
  const needsOwner = () => crForge() === "Bitbucket" || crForge() === "AzureDevOps";
  const needsProject = () => crForge() === "AzureDevOps";

  const createRepo = () => {
    if (!crName().trim()) return;
    if (needsOwner() && !crOwner().trim()) {
      setCrErr("This forge needs an owner (workspace/organization).");
      return;
    }
    setCrBusy(true);
    setCrErr(null);
    setCrResult(null);
    invoke<RepoInfo>("create_remote_repo", {
      forge: crForge(),
      name: crName().trim(),
      private: crPrivate(),
      description: crDesc(),
      owner: crOwner().trim() || null,
      project: crProject().trim() || null,
      addOriginTo: crOrigin() ? props.repoId : null,
    })
      .then((info) => setCrResult(info))
      .catch((e) => setCrErr(String(e)))
      .finally(() => setCrBusy(false));
  };

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
    if (!props.repoId) return;
    invoke<HostingInfo>("hosting_status", { repo: props.repoId })
      .then(setStatus)
      .catch((e) => setErr(String(e)));
  };

  // Global defaults + per-repo overrides (Plan 2). One round-trip returns all
  // three layers (effective / override / global); identity is read separately
  // from the repo's .git/config.
  const [resolved, setResolved] = createSignal<ResolvedRepoSettings | null>(null);
  // Global defaults loaded directly when no repo is open (no resolved layer).
  const [globalOnly, setGlobalOnly] = createSignal<RepoSettings | null>(null);
  const [ident, setIdent] = createSignal<GitIdentity>({ name: null, email: null });
  const [rsErr, setRsErr] = createSignal<string | null>(null);
  const [idName, setIdName] = createSignal("");
  const [idEmail, setIdEmail] = createSignal("");

  const loadRepoSettings = () => {
    const repo = props.repoId;
    if (!repo) {
      // No repo: load just the global defaults so the global section still works.
      setResolved(null);
      globalDefaults().then(setGlobalOnly).catch((e) => setRsErr(String(e)));
      return;
    }
    repoSettings(repo).then(setResolved).catch((e) => setRsErr(String(e)));
    repoIdentityGet(repo)
      .then((i) => {
        setIdent(i);
        setIdName(i.name ?? "");
        setIdEmail(i.email ?? "");
      })
      .catch(() => {});
  };

  // GitHub accounts (work + personal) for multi-account transport. Metadata only;
  // PATs live in the keychain. A repo can be pinned to one via the auth override.
  const [accounts, setAccounts] = createSignal<GitHubAccount[]>([]);
  const [acctName, setAcctName] = createSignal("");
  const [acctEmail, setAcctEmail] = createSignal("");
  const [acctOwners, setAcctOwners] = createSignal("");
  const [acctToken, setAcctToken] = createSignal("");
  const [acctErr, setAcctErr] = createSignal<string | null>(null);
  const [acctBusy, setAcctBusy] = createSignal(false);

  const loadAccounts = () => {
    listGitHubAccounts().then(setAccounts).catch((e) => setAcctErr(String(e)));
  };

  const addAccount = () => {
    if (!acctToken().trim()) return;
    setAcctBusy(true);
    setAcctErr(null);
    const owners = acctOwners()
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    addGitHubAccount(acctName().trim(), acctEmail().trim(), owners, acctToken().trim())
      .then(() => {
        setAcctName("");
        setAcctEmail("");
        setAcctOwners("");
        setAcctToken("");
        loadAccounts();
      })
      .catch((e) => setAcctErr(String(e)))
      .finally(() => setAcctBusy(false));
  };

  const removeAccount = (id: string) => {
    if (!confirm(`Remove GitHub account “${id}”? Repos using it revert to the default credential helper.`)) return;
    removeGitHubAccount(id)
      .then(() => {
        loadAccounts();
        loadRepoSettings();
      })
      .catch((e) => setAcctErr(String(e)));
  };

  const global = (): RepoSettings => resolved()?.global ?? globalOnly() ?? {};
  const override = (): RepoSettings => resolved()?.override ?? {};
  const overridden = (k: keyof RepoSettings) =>
    override()[k] !== undefined && override()[k] !== null;

  const saveGlobal = (patch: Partial<RepoSettings>) => {
    setRsErr(null);
    setGlobalDefaults({ ...global(), ...patch })
      .then(loadRepoSettings)
      .catch((e) => setRsErr(String(e)));
  };
  const saveOverride = (patch: Partial<RepoSettings>) => {
    const repo = props.repoId;
    if (!repo) return;
    setRsErr(null);
    setRepoOverride(repo, { ...override(), ...patch })
      .then(loadRepoSettings)
      .catch((e) => setRsErr(String(e)));
  };
  // Turning an override on seeds it with the current effective value; off clears
  // it (null ⇒ inherit the global default).
  const toggleOverride = <K extends keyof RepoSettings>(
    k: K,
    on: boolean,
    seed: NonNullable<RepoSettings[K]>,
  ) =>
    saveOverride({
      [k]: on ? (resolved()?.effective[k] ?? seed) : null,
    } as Partial<RepoSettings>);

  const saveIdentity = () => {
    const repo = props.repoId;
    if (!repo) return;
    setRsErr(null);
    repoIdentitySet(repo, idName().trim(), idEmail().trim())
      .then(loadRepoSettings)
      .catch((e) => setRsErr(String(e)));
  };

  onMount(() => {
    invoke<LicenseStatus>("license_status").then(setLicense).catch(() => {});
    loadAccounts();
  });

  // Per-repo transport auth override (default / pinned account / SSH key).
  const authKind = (): "default" | "account" | "ssh" => {
    const a = override().auth;
    if (!a) return "default";
    return a.kind === "Account" ? "account" : "ssh";
  };
  const currentAccountId = (): string => {
    const a = override().auth;
    return a && a.kind === "Account" ? a.value : "";
  };
  const currentSshKey = (): string => {
    const a = override().auth;
    return a && a.kind === "SshKey" ? a.value : "";
  };
  const assignAccount = (id: string) => {
    const repo = props.repoId;
    if (!repo || !id) return;
    setRsErr(null);
    assignRepoAccount(repo, id).then(loadRepoSettings).catch((e) => setRsErr(String(e)));
  };
  const setAuth = (auth: RepoAuth | null) => saveOverride({ auth });
  const setAuthMode = (mode: "default" | "account" | "ssh") => {
    if (mode === "default") return setAuth(null);
    if (mode === "ssh") return setAuth({ kind: "SshKey", value: currentSshKey() });
    const id = currentAccountId() || accounts()[0]?.id;
    if (id) assignAccount(id);
    else setAcctErr("Add a GitHub account first (below).");
  };
  // Reload repo-dependent data whenever the active repo changes (the dialog can
  // stay mounted across repo switches, and opens with no repo at all).
  createEffect(() => {
    void props.repoId;
    loadStatus();
    loadRepoSettings();
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
    <div style={{ height: "100%", width: "100%", "box-sizing": "border-box", "overflow-y": "auto", padding: "1rem 1.4rem" }}>
      <h3 style={{ margin: "0 0 0.4rem", "font-size": "0.95rem" }}>Appearance</h3>
      <div style={{ display: "flex", "flex-direction": "column", gap: "0.55rem", "max-width": "30rem" }}>
        <label style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <span style={{ "min-width": "11rem" }}>Color scheme</span>
          <select
            value={themeMode()}
            onChange={(e) => setThemeMode(e.currentTarget.value as ThemeMode)}
            style={{ "font-size": "0.82rem" }}
          >
            <For each={THEME_MODES}>{(m) => <option value={m}>{THEME_LABEL[m]}</option>}</For>
          </select>
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <span style={{ "min-width": "11rem" }}>Accent color</span>
          <select
            value={accent()}
            onChange={(e) => setAccent(e.currentTarget.value as Accent)}
            style={{ "font-size": "0.82rem" }}
          >
            <For each={ACCENTS}>{(a) => <option value={a}>{ACCENT_LABEL[a]}</option>}</For>
          </select>
          <span
            aria-hidden="true"
            style={{ width: "0.9rem", height: "0.9rem", "border-radius": "50%", background: "var(--accent)", display: "inline-block", border: "1px solid var(--bd)" }}
          />
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <span style={{ "min-width": "11rem" }}>Local Changes file layout</span>
          <select
            value={changesLayout()}
            onChange={(e) => setChangesLayout(e.currentTarget.value as "list" | "tree")}
            style={{ "font-size": "0.82rem" }}
          >
            <option value="list">List</option>
            <option value="tree">Tree</option>
          </select>
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <span style={{ "min-width": "11rem" }}>Text size</span>
          <select
            value={textSize()}
            onChange={(e) => setTextSize(e.currentTarget.value as SizeStep)}
            style={{ "font-size": "0.82rem" }}
          >
            <For each={SIZE_OPTIONS}>{(s) => <option value={s}>{SIZE_LABEL[s]}</option>}</For>
          </select>
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <span style={{ "min-width": "11rem" }}>Padding</span>
          <select
            value={uiPadding()}
            onChange={(e) => setUiPadding(e.currentTarget.value as SizeStep)}
            style={{ "font-size": "0.82rem" }}
          >
            <For each={SIZE_OPTIONS}>{(s) => <option value={s}>{SIZE_LABEL[s]}</option>}</For>
          </select>
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.85rem" }}>
          <input
            type="checkbox"
            checked={wrapDiff()}
            onChange={(e) => setWrapDiff(e.currentTarget.checked)}
          />
          Wrap long lines in diffs
        </label>
      </div>

      <h3 style={{ margin: "1.2rem 0 0.4rem", "font-size": "0.95rem" }}>Git defaults</h3>
      <p style={{ "font-size": "0.8rem", color: "var(--fg-muted)", margin: "0 0 0.5rem" }}>
        Used for every repository unless overridden below.
      </p>
      <Show when={rsErr()}>
        <p role="alert" style={{ color: "var(--error)", "font-size": "0.82rem" }}>{rsErr()}</p>
      </Show>
      <div style={{ display: "flex", "flex-direction": "column", gap: "0.5rem", "max-width": "30rem" }}>
        <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.85rem" }}>
          <input
            type="checkbox"
            checked={global().sign ?? BUILTIN_SIGN}
            onChange={(e) => saveGlobal({ sign: e.currentTarget.checked })}
          />
          Sign commits by default
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.85rem" }}>
          <span style={{ "min-width": "11rem" }}>Default merge fast-forward</span>
          <select
            value={global().ff ?? BUILTIN_FF}
            onChange={(e) => saveGlobal({ ff: e.currentTarget.value as FfMode })}
            style={{ "font-size": "0.82rem" }}
          >
            <option value="Auto">Auto (--ff)</option>
            <option value="Only">Only (--ff-only)</option>
            <option value="Never">Never (--no-ff)</option>
          </select>
        </label>
        <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "font-size": "0.85rem" }}>
          <span style={{ "min-width": "11rem" }}>Default base branch</span>
          <input
            style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
            placeholder="(auto-detect main/master)"
            value={global().base_branch ?? ""}
            onChange={(e) => saveGlobal({ base_branch: e.currentTarget.value.trim() || null })}
          />
        </label>
      </div>

      <Show when={props.repoId}>
      <h3 style={{ margin: "1.2rem 0 0.4rem", "font-size": "0.95rem" }}>This repository</h3>
      <p style={{ "font-size": "0.8rem", color: "var(--fg-muted)", margin: "0 0 0.5rem" }}>
        Overrides apply only to this repo; unchecked rows inherit the global default.
      </p>
      <div style={{ display: "flex", "flex-direction": "column", gap: "0.6rem", "max-width": "30rem" }}>
        {/* Commit signing override */}
        <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "min-width": "11rem" }}>
            <input
              type="checkbox"
              checked={overridden("sign")}
              onChange={(e) => toggleOverride("sign", e.currentTarget.checked, !(global().sign ?? BUILTIN_SIGN))}
            />
            Override commit signing
          </label>
          <Show
            when={overridden("sign")}
            fallback={<span style={{ color: "var(--fg-muted)" }}>inherits: {(global().sign ?? BUILTIN_SIGN) ? "on" : "off"}</span>}
          >
            <label style={{ display: "flex", "align-items": "center", gap: "0.3rem" }}>
              <input type="checkbox" checked={!!override().sign} onChange={(e) => saveOverride({ sign: e.currentTarget.checked })} />
              sign commits
            </label>
          </Show>
        </div>

        {/* Fast-forward override */}
        <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "min-width": "11rem" }}>
            <input
              type="checkbox"
              checked={overridden("ff")}
              onChange={(e) => toggleOverride("ff", e.currentTarget.checked, global().ff ?? BUILTIN_FF)}
            />
            Override merge fast-forward
          </label>
          <Show
            when={overridden("ff")}
            fallback={<span style={{ color: "var(--fg-muted)" }}>inherits: {global().ff ?? BUILTIN_FF}</span>}
          >
            <select value={override().ff ?? BUILTIN_FF} onChange={(e) => saveOverride({ ff: e.currentTarget.value as FfMode })} style={{ "font-size": "0.82rem" }}>
              <option value="Auto">Auto</option>
              <option value="Only">Only</option>
              <option value="Never">Never</option>
            </select>
          </Show>
        </div>

        {/* Base branch override */}
        <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "min-width": "11rem" }}>
            <input
              type="checkbox"
              checked={overridden("base_branch")}
              onChange={(e) => toggleOverride("base_branch", e.currentTarget.checked, global().base_branch ?? "main")}
            />
            Override base branch
          </label>
          <Show
            when={overridden("base_branch")}
            fallback={<span style={{ color: "var(--fg-muted)" }}>inherits: {global().base_branch ?? "auto"}</span>}
          >
            <input
              style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
              value={override().base_branch ?? ""}
              onChange={(e) => saveOverride({ base_branch: e.currentTarget.value.trim() || null })}
            />
          </Show>
        </div>

        {/* AI model override */}
        <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
          <label style={{ display: "flex", "align-items": "center", gap: "0.4rem", "min-width": "11rem" }}>
            <input
              type="checkbox"
              checked={overridden("ai_model")}
              onChange={(e) => toggleOverride("ai_model", e.currentTarget.checked, global().ai_model ?? "")}
            />
            Override AI model
          </label>
          <Show
            when={overridden("ai_model")}
            fallback={<span style={{ color: "var(--fg-muted)" }}>inherits: {global().ai_model || "provider default"}</span>}
          >
            <input
              style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
              placeholder="model id"
              value={override().ai_model ?? ""}
              onChange={(e) => saveOverride({ ai_model: e.currentTarget.value.trim() || null })}
            />
          </Show>
        </div>

        {/* Transport auth override (multi-account: default / account / SSH key) */}
        <div style={{ display: "flex", "flex-direction": "column", gap: "0.4rem", "font-size": "0.85rem" }}>
          <label style={{ display: "flex", "align-items": "center", gap: "0.5rem" }}>
            <span style={{ "min-width": "11rem" }}>Push/pull credentials</span>
            <select
              value={authKind()}
              onChange={(e) => setAuthMode(e.currentTarget.value as "default" | "account" | "ssh")}
              style={{ "font-size": "0.82rem" }}
            >
              <option value="default">Default (system git / gh)</option>
              <option value="account">GitHub account (HTTPS)</option>
              <option value="ssh">SSH key</option>
            </select>
          </label>
          <Show when={authKind() === "account"}>
            <label style={{ display: "flex", "align-items": "center", gap: "0.5rem" }}>
              <span style={{ "min-width": "11rem" }}>Account</span>
              <Show
                when={accounts().length > 0}
                fallback={<span style={{ color: "var(--fg-muted)" }}>No accounts yet — add one below.</span>}
              >
                <select
                  value={currentAccountId()}
                  onChange={(e) => assignAccount(e.currentTarget.value)}
                  style={{ "font-size": "0.82rem" }}
                >
                  <For each={accounts()}>{(a) => <option value={a.id}>{a.login}</option>}</For>
                </select>
              </Show>
            </label>
          </Show>
          <Show when={authKind() === "ssh"}>
            <label style={{ display: "flex", "align-items": "center", gap: "0.5rem" }}>
              <span style={{ "min-width": "11rem" }}>SSH private key path</span>
              <input
                style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
                placeholder="/home/you/.ssh/id_work"
                value={currentSshKey()}
                onChange={(e) => setAuth({ kind: "SshKey", value: e.currentTarget.value.trim() })}
              />
            </label>
          </Show>
        </div>

        {/* Git identity (writes .git/config --local) */}
        <div style={{ "border-top": "1px solid var(--border)", "padding-top": "0.6rem", "margin-top": "0.2rem" }}>
          <div style={{ "font-size": "0.85rem", margin: "0 0 0.4rem" }}>
            Git identity <span style={{ color: "var(--fg-muted)" }}>(this repo's .git/config)</span>
          </div>
          <div style={{ display: "flex", "flex-direction": "column", gap: "0.4rem" }}>
            <input
              style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
              placeholder={ident().name ? "" : "user.name (leave blank to inherit global git)"}
              value={idName()}
              onInput={(e) => setIdName(e.currentTarget.value)}
            />
            <input
              style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }}
              placeholder="user.email"
              value={idEmail()}
              onInput={(e) => setIdEmail(e.currentTarget.value)}
            />
            <button onClick={saveIdentity} style={{ "align-self": "flex-start", padding: "0.3rem 0.9rem" }}>
              Save identity
            </button>
          </div>
        </div>
      </div>
      </Show>

      <h3 style={{ margin: "1.2rem 0 0.4rem", "font-size": "0.95rem" }}>GitHub accounts</h3>
      <p style={{ "font-size": "0.8rem", color: "var(--fg-muted)", margin: "0 0 0.5rem" }}>
        Add your work and personal accounts here, then pin a repo to one under
        “This repository” so pushes use the right credentials automatically. Tokens
        are stored in your OS keychain — never on disk or in logs.
      </p>
      <Show when={acctErr()}>
        <p role="alert" style={{ color: "var(--error)", "font-size": "0.82rem" }}>{acctErr()}</p>
      </Show>
      <div style={{ display: "flex", "flex-direction": "column", gap: "0.3rem", "max-width": "30rem" }}>
        <For each={accounts()}>
          {(a) => (
            <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "font-size": "0.85rem" }}>
              <span style={{ flex: "1" }}>
                <strong>{a.login}</strong>
                <Show when={a.email}>
                  <span style={{ color: "var(--fg-muted)" }}> · {a.email}</span>
                </Show>
                <Show when={a.known_owners.length > 0}>
                  <span style={{ color: "var(--fg-muted)" }}> · owners: {a.known_owners.join(", ")}</span>
                </Show>
              </span>
              <button onClick={() => removeAccount(a.id)} style={{ padding: "0.2rem 0.7rem" }}>Remove</button>
            </div>
          )}
        </For>
        <Show when={accounts().length === 0}>
          <p style={{ color: "var(--fg-muted)", "font-size": "0.82rem", margin: "0" }}>No accounts yet.</p>
        </Show>
      </div>
      <div style={{ display: "flex", "flex-direction": "column", gap: "0.4rem", "max-width": "30rem", "margin-top": "0.5rem" }}>
        <input style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder="commit name (user.name)" value={acctName()} onInput={(e) => setAcctName(e.currentTarget.value)} />
        <input style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder="commit email (user.email)" value={acctEmail()} onInput={(e) => setAcctEmail(e.currentTarget.value)} />
        <input style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder="match owners/orgs (comma-separated, optional)" value={acctOwners()} onInput={(e) => setAcctOwners(e.currentTarget.value)} />
        <input type="password" style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder="personal access token" value={acctToken()} onInput={(e) => setAcctToken(e.currentTarget.value)} onKeyDown={(e) => e.key === "Enter" && addAccount()} />
        <button onClick={addAccount} disabled={acctBusy()} style={{ "align-self": "flex-start", padding: "0.3rem 0.9rem" }}>
          {acctBusy() ? "Validating…" : "Add account"}
        </button>
      </div>

      <h3 style={{ margin: "1.2rem 0 0.4rem", "font-size": "0.95rem" }}>Updates</h3>
      <div style={{ display: "flex", gap: "0.4rem", "align-items": "center" }}>
        <button onClick={checkUpdates} disabled={updBusy()} style={{ padding: "0.3rem 0.9rem" }}>
          {updBusy() ? "Working…" : "Check for updates"}
        </button>
        <Show when={update() && !update()!.available}>
          <span style={{ "font-size": "0.82rem", color: "var(--fg-muted)" }}>
            Up to date (v{update()!.current}).
          </span>
        </Show>
      </div>
      <Show when={update()?.available}>
        <div role="status" style={{ margin: "0.5rem 0", padding: "0.5rem 0.7rem", border: "1px solid var(--border)", "border-radius": "6px", background: "var(--surface-2)" }}>
          <p style={{ margin: "0 0 0.3rem", "font-size": "0.85rem" }}>
            <strong>Update available:</strong> v{update()!.version} (you have v{update()!.current})
          </p>
          <Show when={update()!.notes}>
            <p style={{ margin: "0 0 0.4rem", "font-size": "0.8rem", color: "var(--fg-muted)", "white-space": "pre-wrap" }}>{update()!.notes}</p>
          </Show>
          <button onClick={installUpdate} disabled={updBusy()} style={{ padding: "0.3rem 0.9rem" }}>
            {updBusy() ? "Installing…" : "Download & install, then restart"}
          </button>
        </div>
      </Show>
      <Show when={updateErr()}>
        <p role="alert" style={{ color: "var(--error)", "font-size": "0.82rem" }}>{updateErr()}</p>
      </Show>

      <h3 style={{ margin: "1.2rem 0 0.4rem", "font-size": "0.95rem" }}>License</h3>
      <p style={{ "font-size": "0.85rem", color: "var(--fg)", margin: "0 0 0.4rem" }}>{describeLicense(license())}</p>
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
        <p style={{ color: "var(--error)", "font-size": "0.82rem" }}>{licenseErr()}</p>
      </Show>

      <Show when={props.repoId}>
      <h3 style={{ margin: "1.2rem 0 0.6rem", "font-size": "0.95rem" }}>Hosting — {forgeName()}</h3>

      <Show when={err()}>
        <p style={{ color: "var(--error)", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      <Show
        when={status().kind}
        fallback={<p style={{ color: "var(--fg-muted)", "font-size": "0.82rem" }}>No supported forge remote detected.</p>}
      >
        <Show
          when={status().connected}
          fallback={
            <div>
              <p style={{ "font-size": "0.85rem", color: "var(--fg)" }}>
                Connect to {forgeName()} with a personal access token (stored in your OS keychain —
                never on disk or in logs). Used for PRs, notifications, and HTTPS fetch/push/pull.
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
            <span style={{ color: "var(--success)", "font-size": "0.9rem" }}>
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
          fallback={<p style={{ color: "var(--fg-muted)", "font-size": "0.82rem" }}>Could not parse a repo from the remote.</p>}
        >
          <p style={{ "font-family": "monospace", "font-size": "0.85rem" }}>
            {status().slug!.project
              ? `${status().slug!.owner}/${status().slug!.project}/${status().slug!.repo}`
              : `${status().slug!.owner}/${status().slug!.repo}`}
          </p>
        </Show>
      </Show>

      {/* Create remote repository (PH4-005) */}
      <h3 style={{ margin: "1.2rem 0 0.6rem", "font-size": "0.95rem" }}>Create remote repository</h3>
      <Show when={crErr()}>
        <p style={{ color: "var(--error)", "font-size": "0.82rem" }}>{crErr()}</p>
      </Show>
      <Show
        when={crResult()}
        fallback={
          <div style={{ display: "flex", "flex-direction": "column", gap: "0.4rem", "max-width": "30rem" }}>
            <div style={{ display: "flex", gap: "0.4rem", "align-items": "center" }}>
              <select value={crForge()} onChange={(e) => setCrForge(e.currentTarget.value as ForgeKind)} style={{ "font-size": "0.82rem" }}>
                <For each={FORGE_KINDS}>{(k) => <option value={k}>{FORGE_LABEL[k]}</option>}</For>
              </select>
              <input style={{ flex: "1", padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder="repo name" value={crName()} onInput={(e) => setCrName(e.currentTarget.value)} />
            </div>
            <Show when={needsOwner()}>
              <input style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder={crForge() === "AzureDevOps" ? "organization" : "workspace"} value={crOwner()} onInput={(e) => setCrOwner(e.currentTarget.value)} />
            </Show>
            <Show when={needsProject()}>
              <input style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder="project" value={crProject()} onInput={(e) => setCrProject(e.currentTarget.value)} />
            </Show>
            <input style={{ padding: "0.3rem 0.5rem", "font-size": "0.85rem" }} placeholder="description (optional)" value={crDesc()} onInput={(e) => setCrDesc(e.currentTarget.value)} />
            <label style={{ display: "flex", "align-items": "center", gap: "0.3rem", "font-size": "0.82rem" }}>
              <input type="checkbox" checked={crPrivate()} onChange={() => setCrPrivate((v) => !v)} /> private
            </label>
            <label style={{ display: "flex", "align-items": "center", gap: "0.3rem", "font-size": "0.82rem" }}>
              <input type="checkbox" checked={crOrigin()} onChange={() => setCrOrigin((v) => !v)} /> add as origin to this repo
            </label>
            <button onClick={createRepo} disabled={crBusy()} style={{ "align-self": "flex-start", padding: "0.3rem 0.9rem" }}>
              {crBusy() ? "Creating…" : "Create repository"}
            </button>
          </div>
        }
      >
        <div style={{ "font-size": "0.85rem" }}>
          <p style={{ color: "var(--success)" }}>Repository created.</p>
          <p style={{ "font-family": "monospace", "font-size": "0.8rem", "word-break": "break-all" }}>{crResult()!.clone_url}</p>
          <button onClick={() => invoke("open_url", { url: crResult()!.web_url }).catch((e) => setCrErr(String(e)))} style={{ padding: "0.25rem 0.8rem" }}>
            Open in browser
          </button>
          <button onClick={() => setCrResult(null)} style={{ "margin-left": "0.4rem", padding: "0.25rem 0.8rem" }}>
            Create another
          </button>
        </div>
      </Show>
      </Show>

      {/* AI is global config (provider / keys / consent / model); the per-repo
          enable toggle inside appears only when a repo is open. */}
      <AiSettings repoId={props.repoId} />
    </div>
  );
};

export default SettingsView;
