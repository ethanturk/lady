//! GitLab provider. Detection + resolution land in PH4-001; the API calls
//! (merge request, repo create) are implemented in PH4-002 / PH4-005.

use crate::{
    owner_repo_slug, remote_host, Error, ForgeKind, HostingProvider, NewPullRequest, NewRepo,
    RepoInfo, RepoSlug, Result,
};

/// A GitLab REST (v4) API client.
pub struct GitLabClient {
    pub(crate) base_url: String,
    pub(crate) http: reqwest::Client,
}

impl Default for GitLabClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitLabClient {
    /// A client against gitlab.com.
    pub fn new() -> Self {
        Self::with_base_url("https://gitlab.com/api/v4")
    }

    /// A client against a custom API base (tests / self-hosted GitLab).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        GitLabClient {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent("Lady")
                .build()
                .expect("build reqwest client"),
        }
    }
}

#[async_trait::async_trait]
impl HostingProvider for GitLabClient {
    fn kind(&self) -> ForgeKind {
        ForgeKind::GitLab
    }

    fn detect_slug(&self, remote_urls: &[String]) -> Option<RepoSlug> {
        remote_urls.iter().find_map(|u| {
            let host = remote_host(u)?;
            // gitlab.com or any host carrying "gitlab" (self-hosted heuristic).
            (host == "gitlab.com" || host.contains("gitlab")).then(|| owner_repo_slug(u))?
        })
    }

    async fn get_login(&self, token: &str) -> Result<String> {
        let _ = (&self.base_url, &self.http, token); // implemented in PH4-002
        Err(Error::NotImplemented)
    }

    async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String> {
        let _ = (&self.base_url, &self.http, token, slug, pr); // PH4-002
        Err(Error::NotImplemented)
    }

    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo> {
        let _ = (&self.base_url, &self.http, token, repo); // PH4-005
        Err(Error::NotImplemented)
    }
}
