//! Azure DevOps provider. Detection + resolution land in PH4-001; the API calls
//! (pull request, repo create) are implemented in PH4-004 / PH4-005.
//!
//! Azure repos are identified by organization / project / repo. The slug stores
//! org in `owner`, the project in `project`, and the repo in `repo`.

use serde::Deserialize;

use crate::{
    api_error_message, path_segments, remote_host, Error, ForgeKind, HostingProvider,
    NewPullRequest, NewRepo, RepoInfo, RepoSlug, Result, WebTarget,
};

const API_VERSION: &str = "7.1";

#[derive(Deserialize)]
struct AzureProfile {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "emailAddress")]
    email_address: Option<String>,
}

/// An Azure DevOps REST API client. `api_base` serves git/repo APIs
/// (dev.azure.com), `profile_base` serves the profile API (vssps.dev.azure.com);
/// tests point both at one mock server.
pub struct AzureDevOpsClient {
    pub(crate) api_base: String,
    pub(crate) profile_base: String,
    pub(crate) http: reqwest::Client,
}

impl Default for AzureDevOpsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AzureDevOpsClient {
    /// A client against dev.azure.com (+ vssps for profiles).
    pub fn new() -> Self {
        AzureDevOpsClient {
            api_base: "https://dev.azure.com".to_string(),
            profile_base: "https://vssps.dev.azure.com".to_string(),
            http: reqwest::Client::builder()
                .user_agent("Lady")
                .build()
                .expect("build reqwest client"),
        }
    }

    /// A client against a single custom base (tests): both API + profile.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let base = base_url.into();
        AzureDevOpsClient {
            profile_base: base.clone(),
            api_base: base,
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

    fn web_url(&self, web_base: &str, slug: &RepoSlug, target: &WebTarget) -> String {
        let RepoSlug {
            owner,
            repo,
            project,
        } = slug;
        // Azure's repo home is `/{org}/{project}/_git/{repo}`; project falls back
        // to the repo name (the common default). Refs use a `version=GC|GB|GT`
        // query (best-effort — branch/tag names are not URL-encoded here).
        let project = project.as_deref().unwrap_or(repo.as_str());
        let base = format!("{web_base}/{owner}/{project}/_git/{repo}");
        match target {
            WebTarget::Commit(sha) => format!("{base}/commit/{sha}"),
            WebTarget::Branch(branch) => format!("{base}?version=GB{branch}"),
            WebTarget::Tag(tag) => format!("{base}?version=GT{tag}"),
        }
    }

    async fn get_login(&self, token: &str) -> Result<String> {
        // Azure PATs use Basic auth with an empty username.
        let url = format!(
            "{}/_apis/profile/profiles/me?api-version={API_VERSION}",
            self.profile_base
        );
        let resp = self
            .http
            .get(&url)
            .basic_auth("", Some(token))
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::NON_AUTHORITATIVE_INFORMATION
        {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }
        let profile: AzureProfile = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(profile
            .display_name
            .or(profile.email_address)
            .unwrap_or_else(|| "azure".to_string()))
    }

    async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String> {
        let project = slug.project.as_deref().ok_or_else(|| Error::Api {
            status: 0,
            message: "Azure DevOps repo is missing a project".to_string(),
        })?;
        let url = format!(
            "{}/{}/{}/_apis/git/repositories/{}/pullrequests?api-version={API_VERSION}",
            self.api_base, slug.owner, project, slug.repo
        );
        let resp = self
            .http
            .post(&url)
            .basic_auth("", Some(token))
            .json(&serde_json::json!({
                "sourceRefName": format!("refs/heads/{}", pr.head),
                "targetRefName": format!("refs/heads/{}", pr.base),
                "title": pr.title,
                "description": pr.body,
                "isDraft": pr.draft,
            }))
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status: status.as_u16(),
                message: api_error_message(&body),
            });
        }
        // Azure returns the PR object; build the browser URL from its id.
        let body: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        let id = body
            .get("pullRequestId")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| Error::Http("PR response missing pullRequestId".to_string()))?;
        Ok(format!(
            "{}/{}/{}/_git/{}/pullrequest/{}",
            self.api_base, slug.owner, project, slug.repo, id
        ))
    }

    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo> {
        // Azure repos live in an org/project: POST {org}/{project}/_apis/git/
        // repositories. `private` is a project-level setting, not per-repo.
        let org = repo
            .owner
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Api {
                status: 0,
                message: "Azure DevOps needs an organization (owner) to create a repo".to_string(),
            })?;
        let project = repo
            .project
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Api {
                status: 0,
                message: "Azure DevOps needs a project to create a repo".to_string(),
            })?;
        let url = format!(
            "{}/{}/{}/_apis/git/repositories?api-version={API_VERSION}",
            self.api_base, org, project
        );
        let resp = self
            .http
            .post(&url)
            .basic_auth("", Some(token))
            .json(&serde_json::json!({ "name": repo.name }))
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Unauthorized);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status: status.as_u16(),
                message: api_error_message(&body),
            });
        }
        let body: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(RepoInfo {
            clone_url: body
                .get("remoteUrl")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            web_url: body
                .get("webUrl")
                .and_then(|v| v.as_str())
                .or_else(|| body.get("remoteUrl").and_then(|v| v.as_str()))
                .unwrap_or_default()
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_login_returns_display_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_apis/profile/profiles/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "displayName": "Ada Lovelace"
            })))
            .mount(&server)
            .await;
        let c = AzureDevOpsClient::with_base_url(server.uri());
        assert_eq!(c.get_login("pat").await.expect("login"), "Ada Lovelace");
    }

    #[tokio::test]
    async fn create_pull_request_builds_url_and_surfaces_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/myorg/myproj/_apis/git/repositories/myrepo/pullrequests",
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "pullRequestId": 12
            })))
            .mount(&server)
            .await;
        let c = AzureDevOpsClient::with_base_url(server.uri());
        let slug = RepoSlug {
            owner: "myorg".into(),
            repo: "myrepo".into(),
            project: Some("myproj".into()),
        };
        let pr = NewPullRequest {
            head: "feature".into(),
            base: "main".into(),
            title: "Add".into(),
            body: "b".into(),
            draft: false,
        };
        let url = c
            .create_pull_request("pat", &slug, &pr)
            .await
            .expect("create PR");
        assert_eq!(
            url,
            format!("{}/myorg/myproj/_git/myrepo/pullrequest/12", server.uri())
        );

        let server409 = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/myorg/myproj/_apis/git/repositories/myrepo/pullrequests",
            ))
            .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
                "message": "An active pull request already exists."
            })))
            .mount(&server409)
            .await;
        let c409 = AzureDevOpsClient::with_base_url(server409.uri());
        match c409
            .create_pull_request("pat", &slug, &pr)
            .await
            .unwrap_err()
        {
            Error::Api { status, message } => {
                assert_eq!(status, 409);
                assert!(message.contains("already exists"), "msg: {message}");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_repo_in_project_returns_urls() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/myorg/myproj/_apis/git/repositories"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "remoteUrl": "https://dev.azure.com/myorg/myproj/_git/newrepo",
                "webUrl": "https://dev.azure.com/myorg/myproj/_git/newrepo"
            })))
            .mount(&server)
            .await;
        let c = AzureDevOpsClient::with_base_url(server.uri());
        let info = c
            .create_repo(
                "pat",
                &NewRepo {
                    name: "newrepo".into(),
                    private: true,
                    description: "d".into(),
                    owner: Some("myorg".into()),
                    project: Some("myproj".into()),
                },
            )
            .await
            .expect("create repo");
        assert_eq!(
            info.clone_url,
            "https://dev.azure.com/myorg/myproj/_git/newrepo"
        );
    }

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
