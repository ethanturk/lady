// Global-default + per-repo-override settings (Plan 2). Most settings are
// global; the git-ish ones (commit signing, merge fast-forward policy, default
// base branch, AI model) and the git identity can be overridden per repo.
//
// The backend resolves `effective = repo override ?? global default`; the
// built-in fallbacks (sign=false, ff=Auto) are applied at the point of use so a
// freshly-loaded repo still has a sensible starting value.
import { invoke } from "@tauri-apps/api/core";
import type {
  FfMode,
  GitIdentity,
  RepoId,
  RepoSettings,
  ResolvedRepoSettings,
} from "./commands";

/** Built-in fallbacks for fields left unset both per-repo and globally. */
export const BUILTIN_SIGN = false;
export const BUILTIN_FF: FfMode = "Auto";

/** All three layers (effective / override / global) in one round-trip. */
export const repoSettings = (repo: RepoId): Promise<ResolvedRepoSettings> =>
  invoke("repo_settings", { repo });

/** Replace this repo's override block (an all-empty block clears the override). */
export const setRepoOverride = (
  repo: RepoId,
  settings: RepoSettings,
): Promise<void> => invoke("set_repo_override", { repo, settings });

/** Read the global defaults block (no repo needed). */
export const globalDefaults = (): Promise<RepoSettings> =>
  invoke("global_defaults");

/** Replace the global defaults block. */
export const setGlobalDefaults = (settings: RepoSettings): Promise<void> =>
  invoke("set_global_defaults", { settings });

/** Read the repo's local git identity (`.git/config`). */
export const repoIdentityGet = (repo: RepoId): Promise<GitIdentity> =>
  invoke("repo_identity_get", { repo });

/** Write the repo's local git identity. Empty strings unset the keys. */
export const repoIdentitySet = (
  repo: RepoId,
  name: string,
  email: string,
): Promise<void> => invoke("repo_identity_set", { repo, name, email });

/** Effective commit-signing default for `repo`, with the built-in fallback. */
export const effectiveSign = (r: ResolvedRepoSettings): boolean =>
  r.effective.sign ?? BUILTIN_SIGN;

/** Effective fast-forward default for `repo`, with the built-in fallback. */
export const effectiveFf = (r: ResolvedRepoSettings): FfMode =>
  r.effective.ff ?? BUILTIN_FF;
