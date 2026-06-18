//! `lady-hosting` — forge integration for Lady (PLAN.md §7).
//!
//! A forge-agnostic [`HostingProvider`] trait with one implementation per forge
//! (GitHub, GitLab, Bitbucket, Azure DevOps). [`provider_for`] resolves a git
//! remote URL — or a self-hosted base from config — to the right provider.
//!
//! Tokens live in the OS keychain (ADR-0006) via [`TokenStore`], under a
//! per-forge key ([`HostingProvider::token_key`]) — never on disk, in
//! plaintext, or in logs. The transport/signing credentials for git itself stay
//! with system git; only hosting-API tokens are stored here.

use serde::{Deserialize, Serialize};

mod azure;
mod bitbucket;
mod github;
mod gitlab;

pub use azure::AzureDevOpsClient;
pub use bitbucket::BitbucketClient;
pub use github::GitHubClient;
pub use gitlab::GitLabClient;

/// Errors surfaced by hosting operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No matching forge remote could be detected on the repository.
    #[error("no supported forge remote found")]
    NoRemote,
    /// Authentication failed or no token is stored.
    #[error("not authenticated")]
    Unauthorized,
    /// The token store (keychain) failed.
    #[error("token store error: {0}")]
    Store(String),
    /// A network/HTTP error talking to the API.
    #[error("http error: {0}")]
    Http(String),
    /// The API returned an error payload.
    #[error("API error ({status}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Best-effort error message from the API body.
        message: String,
    },
    /// This operation is not implemented for the resolved provider yet.
    #[error("operation not supported by this provider")]
    NotImplemented,
}

/// Result alias for hosting operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Which forge a provider talks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForgeKind {
    GitHub,
    GitLab,
    Bitbucket,
    AzureDevOps,
}

impl ForgeKind {
    /// A short lowercase label (used in UI copy and remote detection).
    pub fn label(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "GitHub",
            ForgeKind::GitLab => "GitLab",
            ForgeKind::Bitbucket => "Bitbucket",
            ForgeKind::AzureDevOps => "Azure DevOps",
        }
    }

    /// The keychain key under which this forge's token is stored (ADR-0006).
    pub fn token_key(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "github-token",
            ForgeKind::GitLab => "gitlab-token",
            ForgeKind::Bitbucket => "bitbucket-token",
            ForgeKind::AzureDevOps => "azure-token",
        }
    }
}

/// A repository identifier on a forge. `owner`/`repo` for GitHub/GitLab/
/// Bitbucket; Azure DevOps additionally carries `project` (the slug is
/// org=`owner` / project / repo).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSlug {
    /// The owner (user / org / Azure organization).
    pub owner: String,
    /// The repository name.
    pub repo: String,
    /// Azure DevOps project (the middle path segment); `None` elsewhere.
    #[serde(default)]
    pub project: Option<String>,
}

impl RepoSlug {
    /// A simple owner/repo slug (no Azure project).
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        RepoSlug {
            owner: owner.into(),
            repo: repo.into(),
            project: None,
        }
    }
}

/// Details for opening a pull / merge request.
#[derive(Clone, Debug, Serialize)]
pub struct NewPullRequest {
    /// The branch with the changes (source).
    pub head: String,
    /// The branch to merge into (target).
    pub base: String,
    /// Title.
    pub title: String,
    /// Body / description (markdown).
    pub body: String,
    /// Whether to open as a draft.
    pub draft: bool,
}

/// Details for creating a remote repository (PH4-005).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NewRepo {
    /// Repository name.
    pub name: String,
    /// Whether the repo is private.
    pub private: bool,
    /// Optional description.
    pub description: String,
    /// Owning org / workspace. Required for Bitbucket (workspace) and Azure
    /// (organization); optional for GitHub (an org) and ignored by GitLab.
    #[serde(default)]
    pub owner: Option<String>,
    /// Azure DevOps project (required for Azure); ignored elsewhere.
    #[serde(default)]
    pub project: Option<String>,
}

/// The created remote repository's URLs (PH4-005).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoInfo {
    /// URL to clone from (https).
    pub clone_url: String,
    /// URL to view in a browser.
    pub web_url: String,
}

/// A forge-agnostic hosting provider. One implementation per forge; resolved
/// from a remote by [`provider_for`].
#[async_trait::async_trait]
pub trait HostingProvider: Send + Sync {
    /// Which forge this provider talks to.
    fn kind(&self) -> ForgeKind;

    /// The keychain key for this forge's token (ADR-0006).
    fn token_key(&self) -> &'static str {
        self.kind().token_key()
    }

    /// Detect this provider's repo slug among the repository's remote URLs.
    fn detect_slug(&self, remote_urls: &[String]) -> Option<RepoSlug>;

    /// Verify `token` and return the authenticated user's login/handle.
    async fn get_login(&self, token: &str) -> Result<String>;

    /// Open a pull / merge request, returning its web URL.
    async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String>;

    /// Create a remote repository, returning its clone + web URLs (PH4-005).
    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo>;
}

// ── Remote-URL parsing & provider resolution ────────────────────────────────────

/// Extract the host from a git remote URL (https or scp-like ssh).
pub fn remote_host(url: &str) -> Option<String> {
    let u = url.trim();
    if let Some((_, after)) = u.split_once("://") {
        // scheme://[user@]host[:port]/path
        let authority = after.split('/').next()?;
        let authority = authority.rsplit('@').next()?; // drop any userinfo
        let host = authority.split(':').next()?; // drop any port
        return (!host.is_empty()).then(|| host.to_string());
    }
    // scp-like: [user@]host:path
    if let Some((left, _)) = u.split_once(':') {
        let host = left.rsplit('@').next()?;
        return host.contains('.').then(|| host.to_string());
    }
    None
}

/// The path segments after the host (no leading/trailing slash, `.git` stripped
/// from the last one).
pub(crate) fn path_segments(url: &str) -> Vec<String> {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    // Everything after the host: take the path portion.
    let path = if let Some((_, after)) = u.split_once("://") {
        after.split_once('/').map(|(_, p)| p).unwrap_or("")
    } else if let Some((_, after)) = u.split_once(':') {
        after // scp-like `host:owner/repo`
    } else {
        ""
    };
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// A configured self-hosted forge: map a `host` to its `kind` and API `base`.
#[derive(Clone, Debug)]
pub struct ForgeConfig {
    /// The remote host (e.g. `gitlab.mycorp.com`).
    pub host: String,
    /// Which forge software it runs.
    pub kind: ForgeKind,
    /// The API base URL (e.g. `https://gitlab.mycorp.com/api/v4`).
    pub api_base: String,
}

/// Build the public-host provider for `kind` (PH4-005 create-repo, where there
/// is no remote URL to resolve from).
pub fn provider_by_kind(kind: ForgeKind) -> Box<dyn HostingProvider> {
    match kind {
        ForgeKind::GitHub => Box::new(GitHubClient::new()),
        ForgeKind::GitLab => Box::new(GitLabClient::new()),
        ForgeKind::Bitbucket => Box::new(BitbucketClient::new()),
        ForgeKind::AzureDevOps => Box::new(AzureDevOpsClient::new()),
    }
}

/// Build a provider of `kind` against `api_base`.
fn provider_of(kind: ForgeKind, api_base: &str) -> Box<dyn HostingProvider> {
    match kind {
        ForgeKind::GitHub => Box::new(GitHubClient::with_base_url(api_base)),
        ForgeKind::GitLab => Box::new(GitLabClient::with_base_url(api_base)),
        ForgeKind::Bitbucket => Box::new(BitbucketClient::with_base_url(api_base)),
        ForgeKind::AzureDevOps => Box::new(AzureDevOpsClient::with_base_url(api_base)),
    }
}

/// Resolve a remote URL to a hosting provider. `extra` self-hosted configs are
/// tried first (by exact host match), then the public hosts.
pub fn provider_for(remote_url: &str, extra: &[ForgeConfig]) -> Option<Box<dyn HostingProvider>> {
    let host = remote_host(remote_url)?;

    if let Some(cfg) = extra.iter().find(|c| c.host == host) {
        return Some(provider_of(cfg.kind, &cfg.api_base));
    }

    let kind = match host.as_str() {
        "github.com" => ForgeKind::GitHub,
        "gitlab.com" => ForgeKind::GitLab,
        "bitbucket.org" => ForgeKind::Bitbucket,
        h if h == "dev.azure.com"
            || h == "ssh.dev.azure.com"
            || h.ends_with(".visualstudio.com") =>
        {
            ForgeKind::AzureDevOps
        }
        _ => return None,
    };
    Some(match kind {
        ForgeKind::GitHub => Box::new(GitHubClient::new()),
        ForgeKind::GitLab => Box::new(GitLabClient::new()),
        ForgeKind::Bitbucket => Box::new(BitbucketClient::new()),
        ForgeKind::AzureDevOps => Box::new(AzureDevOpsClient::new()),
    })
}

// ── Generic slug helpers (shared by the simple owner/repo forges) ────────────────

/// owner = first path segment, repo = last path segment (handles GitLab
/// subgroups by collapsing the middle into the repo name's group path is left
/// to per-forge logic; here owner/repo are the outer bounds).
pub(crate) fn owner_repo_slug(url: &str) -> Option<RepoSlug> {
    let segs = path_segments(url);
    if segs.len() < 2 {
        return None;
    }
    Some(RepoSlug::new(segs.first()?.clone(), segs.last()?.clone()))
}

/// Parse a GitHub remote URL into a [`RepoSlug`] (back-compat helper).
pub fn parse_github_remote(url: &str) -> Option<RepoSlug> {
    let host = remote_host(url)?;
    if host != "github.com" {
        return None;
    }
    owner_repo_slug(url)
}

/// Detect the first GitHub repository among a list of remote URLs (back-compat).
pub fn detect_github_slug(remote_urls: &[String]) -> Option<RepoSlug> {
    remote_urls.iter().find_map(|u| parse_github_remote(u))
}

// ── Token store ────────────────────────────────────────────────────────────────

/// Abstraction over secure token storage so the keychain can be mocked in tests.
/// Implementations must never log or persist tokens in plaintext.
pub trait TokenStore: Send + Sync {
    /// Fetch the stored token for `key`, or `None` when absent.
    fn get(&self, key: &str) -> Result<Option<String>>;
    /// Store `value` for `key`.
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// Delete any token stored for `key` (idempotent).
    fn delete(&self, key: &str) -> Result<()>;
}

/// OS-keychain-backed token store via the `keyring` crate (ADR-0006).
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    /// Create a store under the given keychain service name.
    pub fn new(service: impl Into<String>) -> Self {
        KeyringStore {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key).map_err(|e| Error::Store(e.to_string()))
    }
}

impl TokenStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Store(e.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| Error::Store(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Store(e.to_string())),
        }
    }
}

/// Shared helper: extract `{ "message": ... }` from a JSON error body, falling
/// back to the raw body. Used by the per-forge clients.
pub(crate) fn api_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .and_then(|m| m.as_str())
                .map(String::from)
                // GitLab: `{ "error": "..." }`.
                .or_else(|| v.get("error").and_then(|m| m.as_str()).map(String::from))
                // Bitbucket / Azure: `{ "error": { "message": "..." } }`.
                .or_else(|| {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(String::from)
                })
        })
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_host_handles_https_and_ssh() {
        assert_eq!(
            remote_host("https://github.com/o/r.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            remote_host("git@github.com:o/r.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            remote_host("ssh://git@gitlab.com/o/r.git").as_deref(),
            Some("gitlab.com")
        );
        assert_eq!(
            remote_host("https://user@bitbucket.org/o/r").as_deref(),
            Some("bitbucket.org")
        );
        assert_eq!(
            remote_host("https://dev.azure.com/org/proj/_git/repo").as_deref(),
            Some("dev.azure.com")
        );
    }

    #[test]
    fn provider_for_resolves_all_four_public_forges() {
        let cases = [
            ("https://github.com/o/r.git", ForgeKind::GitHub),
            ("git@gitlab.com:o/r.git", ForgeKind::GitLab),
            ("https://bitbucket.org/o/r.git", ForgeKind::Bitbucket),
            (
                "https://dev.azure.com/org/proj/_git/repo",
                ForgeKind::AzureDevOps,
            ),
            (
                "git@ssh.dev.azure.com:v3/org/proj/repo",
                ForgeKind::AzureDevOps,
            ),
            (
                "https://myorg.visualstudio.com/proj/_git/repo",
                ForgeKind::AzureDevOps,
            ),
        ];
        for (url, want) in cases {
            let p = provider_for(url, &[]).unwrap_or_else(|| panic!("resolve {url}"));
            assert_eq!(p.kind(), want, "url {url}");
        }
        // A non-forge remote resolves to nothing.
        assert!(provider_for("https://example.com/x/y.git", &[]).is_none());
    }

    #[test]
    fn provider_for_resolves_self_hosted_from_config() {
        let extra = [ForgeConfig {
            host: "gitlab.mycorp.com".to_string(),
            kind: ForgeKind::GitLab,
            api_base: "https://gitlab.mycorp.com/api/v4".to_string(),
        }];
        let p = provider_for("https://gitlab.mycorp.com/group/proj.git", &extra)
            .expect("resolve self-hosted");
        assert_eq!(p.kind(), ForgeKind::GitLab);
        // Without the config it would be unknown.
        assert!(provider_for("https://gitlab.mycorp.com/group/proj.git", &[]).is_none());
    }

    #[test]
    fn per_forge_token_keys_are_distinct() {
        let keys = [
            ForgeKind::GitHub.token_key(),
            ForgeKind::GitLab.token_key(),
            ForgeKind::Bitbucket.token_key(),
            ForgeKind::AzureDevOps.token_key(),
        ];
        let set: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(set.len(), 4, "token keys must be distinct: {keys:?}");
    }
}
