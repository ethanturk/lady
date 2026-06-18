//! Azure DevOps provider. Detection + resolution land in PH4-001; the API calls
//! (pull request, repo create) are implemented in PH4-004 / PH4-005.
//!
//! Azure repos are identified by organization / project / repo. The slug stores
//! org in `owner`, the project in `project`, and the repo in `repo`.

use crate::{
    path_segments, remote_host, Error, ForgeKind, HostingProvider, NewPullRequest, NewRepo,
    RepoInfo, RepoSlug, Result,
};

/// An Azure DevOps REST API client.
pub struct AzureDevOpsClient {
    pub(crate) base_url: String,
    pub(crate) http: reqwest::Client,
}

impl Default for AzureDevOpsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AzureDevOpsClient {
    /// A client against dev.azure.com.
    pub fn new() -> Self {
        Self::with_base_url("https://dev.azure.com")
    }

    /// A client against a custom API base (tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        AzureDevOpsClient {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent("Lady")
                .build()
                .expect("build reqwest client"),
        }
    }
}

/// Parse an Azure DevOps remote into org/project/repo.
pub(crate) fn azure_slug(url: &str) -> Option<RepoSlug> {
    let host = remote_host(url)?;
    let segs = path_segments(url);

    // `{org}.visualstudio.com/{project}/_git/{repo}` — org is the subdomain.
    if let Some(org) = host.strip_suffix(".visualstudio.com") {
        let repo = repo_after_git(&segs)?;
        let project = segs.first()?.clone();
        return Some(slug(org, &project, &repo));
    }

    // ssh: `ssh.dev.azure.com:v3/{org}/{project}/{repo}` (no `_git`).
    if host == "ssh.dev.azure.com" {
        let mut it = segs.iter();
        if it.next().map(String::as_str) != Some("v3") {
            return None;
        }
        let org = it.next()?;
        let project = it.next()?;
        let repo = it.next()?;
        return Some(slug(org, project, repo));
    }

    // https: `dev.azure.com/{org}/{project}/_git/{repo}`.
    if host == "dev.azure.com" {
        let org = segs.first()?;
        let project = segs.get(1)?;
        let repo = repo_after_git(&segs)?;
        return Some(slug(org, project, &repo));
    }

    None
}

/// The repo name following a `_git` segment (or the last segment).
fn repo_after_git(segs: &[String]) -> Option<String> {
    if let Some(i) = segs.iter().position(|s| s == "_git") {
        segs.get(i + 1).cloned()
    } else {
        segs.last().cloned()
    }
}

fn slug(org: &str, project: &str, repo: &str) -> RepoSlug {
    RepoSlug {
        owner: org.to_string(),
        repo: repo.to_string(),
        project: Some(project.to_string()),
    }
}

#[async_trait::async_trait]
impl HostingProvider for AzureDevOpsClient {
    fn kind(&self) -> ForgeKind {
        ForgeKind::AzureDevOps
    }

    fn detect_slug(&self, remote_urls: &[String]) -> Option<RepoSlug> {
        remote_urls.iter().find_map(|u| azure_slug(u))
    }

    async fn get_login(&self, token: &str) -> Result<String> {
        let _ = (&self.base_url, &self.http, token); // implemented in PH4-004
        Err(Error::NotImplemented)
    }

    async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String> {
        let _ = (&self.base_url, &self.http, token, slug, pr); // PH4-004
        Err(Error::NotImplemented)
    }

    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo> {
        let _ = (&self.base_url, &self.http, token, repo); // PH4-005
        Err(Error::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_azure_remote_shapes() {
        let https = azure_slug("https://dev.azure.com/myorg/myproj/_git/myrepo").unwrap();
        assert_eq!(https.owner, "myorg");
        assert_eq!(https.project.as_deref(), Some("myproj"));
        assert_eq!(https.repo, "myrepo");

        let ssh = azure_slug("git@ssh.dev.azure.com:v3/myorg/myproj/myrepo").unwrap();
        assert_eq!(ssh.owner, "myorg");
        assert_eq!(ssh.project.as_deref(), Some("myproj"));
        assert_eq!(ssh.repo, "myrepo");

        let vs = azure_slug("https://myorg.visualstudio.com/myproj/_git/myrepo").unwrap();
        assert_eq!(vs.owner, "myorg");
        assert_eq!(vs.project.as_deref(), Some("myproj"));
        assert_eq!(vs.repo, "myrepo");
    }
}
