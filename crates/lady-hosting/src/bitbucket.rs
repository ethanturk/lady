//! Bitbucket provider. Detection + resolution land in PH4-001; the API calls
//! (pull request, repo create) are implemented in PH4-003 / PH4-005.

use crate::{
    owner_repo_slug, remote_host, Error, ForgeKind, HostingProvider, NewPullRequest, NewRepo,
    RepoInfo, RepoSlug, Result,
};

/// A Bitbucket Cloud REST (2.0) API client.
pub struct BitbucketClient {
    pub(crate) base_url: String,
    pub(crate) http: reqwest::Client,
}

impl Default for BitbucketClient {
    fn default() -> Self {
        Self::new()
    }
}

impl BitbucketClient {
    /// A client against bitbucket.org.
    pub fn new() -> Self {
        Self::with_base_url("https://api.bitbucket.org/2.0")
    }

    /// A client against a custom API base (tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        BitbucketClient {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent("Lady")
                .build()
                .expect("build reqwest client"),
        }
    }
}

#[async_trait::async_trait]
impl HostingProvider for BitbucketClient {
    fn kind(&self) -> ForgeKind {
        ForgeKind::Bitbucket
    }

    fn detect_slug(&self, remote_urls: &[String]) -> Option<RepoSlug> {
        remote_urls.iter().find_map(|u| {
            let host = remote_host(u)?;
            (host == "bitbucket.org").then(|| owner_repo_slug(u))?
        })
    }

    async fn get_login(&self, token: &str) -> Result<String> {
        let _ = (&self.base_url, &self.http, token); // implemented in PH4-003
        Err(Error::NotImplemented)
    }

    async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String> {
        let _ = (&self.base_url, &self.http, token, slug, pr); // PH4-003
        Err(Error::NotImplemented)
    }

    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo> {
        let _ = (&self.base_url, &self.http, token, repo); // PH4-005
        Err(Error::NotImplemented)
    }
}
