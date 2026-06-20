// Multiple GitHub accounts (work + personal). Accounts are metadata only — the
// PAT lives in the OS keychain. A repo can be pinned to an account so git
// transport (push/pull/fetch/clone) uses the right credentials silently,
// without per-repo `gh` configuration. Repos with no pin keep using the system
// git / `gh auth` credential helper (the default).
import { invoke } from "@tauri-apps/api/core";
import type { AccountSuggestion, GitHubAccount, RepoId } from "./commands";

/** List the registered GitHub accounts (no tokens are ever returned). */
export const listGitHubAccounts = (): Promise<GitHubAccount[]> =>
  invoke("list_github_accounts");

/**
 * Register or update a GitHub account: the PAT is validated against the API to
 * learn the login, then stored in the keychain. Re-adding a login refreshes it.
 */
export const addGitHubAccount = (
  name: string,
  email: string,
  knownOwners: string[],
  token: string,
): Promise<GitHubAccount> =>
  invoke("add_github_account", {
    name,
    email,
    knownOwners,
    token,
  });

/** Remove an account: deletes its token and unpins any repos using it. */
export const removeGitHubAccount = (id: string): Promise<void> =>
  invoke("remove_github_account", { id });

/**
 * Suggest an account for `repo` from the remote owner. Returns `null` when the
 * repo is already pinned, the suggestion was dismissed, or nothing matches — so
 * the UI only ever prompts once.
 */
export const suggestRepoAccount = (
  repo: RepoId,
): Promise<AccountSuggestion | null> => invoke("suggest_repo_account", { repo });

/** Pin `repo` to an account (sets the HTTPS override + stamps the identity). */
export const assignRepoAccount = (
  repo: RepoId,
  accountId: string,
): Promise<void> => invoke("assign_repo_account", { repo, accountId });

/** Never offer the account suggestion for `repo` again. */
export const dismissRepoAccountSuggestion = (repo: RepoId): Promise<void> =>
  invoke("dismiss_repo_account_suggestion", { repo });
