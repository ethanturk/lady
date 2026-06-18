//! `lady-hosting` — forge integration for Lady (GitHub only at v1, PLAN.md §7).
//!
//! Provides a [`HostingProvider`] trait + a GitHub implementation over
//! `reqwest`/`rustls`, GitHub-remote detection, and a [`TokenStore`] abstraction
//! so hosting-API tokens live in the OS keychain (ADR-0006) — never on disk,
//! in plaintext, or in logs. The signing/transport credentials for git itself
//! stay with system git; only hosting-API tokens are stored here.

use serde::{Deserialize, Serialize};

/// Errors surfaced by hosting operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No GitHub remote could be detected on the repository.
    #[error("no GitHub remote found")]
    NoGitHubRemote,
    /// Authentication failed or no token is stored.
    #[error("not authenticated with GitHub")]
    Unauthorized,
    /// The token store (keychain) failed.
    #[error("token store error: {0}")]
    Store(String),
    /// A network/HTTP error talking to the API.
    #[error("http error: {0}")]
    Http(String),
    /// The API returned an error payload.
    #[error("GitHub API error ({status}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Best-effort error message from the API body.
        message: String,
    },
}

/// Result alias for hosting operations.
pub type Result<T> = std::result::Result<T, Error>;

/// An owner/repo pair identifying a repository on a forge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSlug {
    /// The repository owner (user or org).
    pub owner: String,
    /// The repository name.
    pub repo: String,
}

/// Parse a GitHub remote URL (https or ssh) into a [`RepoSlug`]. Returns `None`
/// for non-GitHub or unparseable remotes.
pub fn parse_github_remote(url: &str) -> Option<RepoSlug> {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    // Only github.com is supported at v1 (enterprise is Fast-follow).
    let idx = u.find("github.com")?;
    let rest = &u[idx + "github.com".len()..];
    // After the host comes ':' (ssh) or '/' (https), then owner/repo.
    let rest = rest.trim_start_matches([':', '/']);
    let mut parts = rest.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(RepoSlug {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

/// Detect the first GitHub repository among a list of remote URLs.
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

// ── GitHub client ──────────────────────────────────────────────────────────────

/// The authenticated GitHub user (only the fields Lady needs).
#[derive(Clone, Debug, Deserialize)]
pub struct GitHubUser {
    /// The account's login handle.
    pub login: String,
}

/// Details for opening a pull request (PH3-012).
#[derive(Clone, Debug, Serialize)]
pub struct NewPullRequest {
    /// The branch with the changes.
    pub head: String,
    /// The branch to merge into.
    pub base: String,
    /// PR title.
    pub title: String,
    /// PR body (markdown).
    pub body: String,
    /// Whether to open as a draft.
    pub draft: bool,
}

/// A minimal hosting provider surface.
pub trait HostingProvider {
    /// Detect this provider's repo slug among the repo's remote URLs.
    fn detect(&self, remote_urls: &[String]) -> Option<RepoSlug>;
}

/// A GitHub REST API client (reqwest + rustls). `base_url` is overridable so
/// tests can point it at a mock server.
pub struct GitHubClient {
    base_url: String,
    http: reqwest::Client,
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubClient {
    /// A client against the public GitHub API.
    pub fn new() -> Self {
        Self::with_base_url("https://api.github.com")
    }

    /// A client against a custom base URL (for tests / GitHub Enterprise).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        GitHubClient {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent("Lady")
                .build()
                .expect("build reqwest client"),
        }
    }

    /// Verify `token` and return the authenticated user (`GET /user`).
    pub async fn get_user(&self, token: &str) -> Result<GitHubUser> {
        let url = format!("{}/user", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        resp.json::<GitHubUser>()
            .await
            .map_err(|e| Error::Http(e.to_string()))
    }

    /// Open a pull request on `slug` (`POST /repos/{owner}/{repo}/pulls`),
    /// returning its HTML URL (PH3-012).
    pub async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String> {
        let url = format!("{}/repos/{}/{}/pulls", self.base_url, slug.owner, slug.repo);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .json(pr)
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            // GitHub returns `{ "message": "...", "errors": [...] }`.
            let body = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
                .unwrap_or(body);
            return Err(Error::Api {
                status: status.as_u16(),
                message,
            });
        }
        let body: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        body.get("html_url")
            .and_then(|u| u.as_str())
            .map(String::from)
            .ok_or_else(|| Error::Http("PR response missing html_url".to_string()))
    }
}

impl HostingProvider for GitHubClient {
    fn detect(&self, remote_urls: &[String]) -> Option<RepoSlug> {
        detect_github_slug(remote_urls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// In-memory token store for tests (mocks the keychain).
    #[derive(Default)]
    struct MemStore(Mutex<std::collections::HashMap<String, String>>);
    impl TokenStore for MemStore {
        fn get(&self, key: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<()> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[test]
    fn parses_https_and_ssh_github_remotes() {
        let want = RepoSlug {
            owner: "octocat".into(),
            repo: "hello".into(),
        };
        assert_eq!(
            parse_github_remote("https://github.com/octocat/hello.git"),
            Some(want.clone())
        );
        assert_eq!(
            parse_github_remote("git@github.com:octocat/hello.git"),
            Some(want.clone())
        );
        assert_eq!(
            parse_github_remote("https://github.com/octocat/hello"),
            Some(want.clone())
        );
        assert_eq!(
            parse_github_remote("ssh://git@github.com/octocat/hello.git"),
            Some(want)
        );
        assert_eq!(parse_github_remote("https://gitlab.com/x/y.git"), None);
    }

    #[test]
    fn detects_github_among_remotes() {
        let remotes = vec![
            "https://gitlab.com/a/b.git".to_string(),
            "git@github.com:owner/repo.git".to_string(),
        ];
        let slug = detect_github_slug(&remotes).expect("detect github");
        assert_eq!(slug.owner, "owner");
        assert_eq!(slug.repo, "repo");
    }

    #[test]
    fn token_store_abstraction_roundtrips() {
        let store = MemStore::default();
        assert_eq!(store.get("github").unwrap(), None);
        store.set("github", "ghp_secret").unwrap();
        assert_eq!(store.get("github").unwrap().as_deref(), Some("ghp_secret"));
        store.delete("github").unwrap();
        assert_eq!(store.get("github").unwrap(), None);
    }

    #[tokio::test]
    async fn get_user_returns_login_on_200_and_unauthorized_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "octocat"
            })))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri());
        let user = client.get_user("good-token").await.expect("get_user 200");
        assert_eq!(user.login, "octocat");

        // A fresh server with a 401 for the unauthorized path.
        let server401 = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server401)
            .await;
        let client401 = GitHubClient::with_base_url(server401.uri());
        let err = client401.get_user("bad").await.unwrap_err();
        assert!(
            matches!(err, Error::Unauthorized),
            "401 → Unauthorized, got {err:?}"
        );
    }

    #[tokio::test]
    async fn create_pull_request_returns_url_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/octocat/hello/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "html_url": "https://github.com/octocat/hello/pull/42"
            })))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri());
        let slug = RepoSlug {
            owner: "octocat".into(),
            repo: "hello".into(),
        };
        let pr = NewPullRequest {
            head: "feature".into(),
            base: "main".into(),
            title: "Add feature".into(),
            body: "Body".into(),
            draft: false,
        };
        let url = client
            .create_pull_request("tok", &slug, &pr)
            .await
            .expect("create PR");
        assert_eq!(url, "https://github.com/octocat/hello/pull/42");
    }

    #[tokio::test]
    async fn create_pull_request_surfaces_api_error_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/octocat/hello/pulls"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "A pull request already exists for octocat:feature."
            })))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri());
        let slug = RepoSlug {
            owner: "octocat".into(),
            repo: "hello".into(),
        };
        let pr = NewPullRequest {
            head: "feature".into(),
            base: "main".into(),
            title: "t".into(),
            body: "b".into(),
            draft: false,
        };
        let err = client
            .create_pull_request("tok", &slug, &pr)
            .await
            .unwrap_err();
        match err {
            Error::Api { status, message } => {
                assert_eq!(status, 422);
                assert!(message.contains("already exists"), "message: {message}");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }
}
