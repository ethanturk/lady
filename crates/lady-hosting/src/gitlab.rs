//! GitLab provider. Detection + resolution land in PH4-001; the API calls
//! (merge request, repo create) are implemented in PH4-002 / PH4-005.

use serde::Deserialize;

use crate::{
    api_error_message, owner_repo_slug, remote_host, Error, ForgeKind, HostingProvider,
    NewPullRequest, NewRepo, RepoInfo, RepoSlug, Result, WebTarget,
};

#[derive(Deserialize)]
struct GitLabUser {
    username: String,
}

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

    fn web_url(&self, web_base: &str, slug: &RepoSlug, target: &WebTarget) -> String {
        let RepoSlug { owner, repo, .. } = slug;
        match target {
            WebTarget::Commit(sha) => format!("{web_base}/{owner}/{repo}/-/commit/{sha}"),
            WebTarget::Branch(branch) => format!("{web_base}/{owner}/{repo}/-/tree/{branch}"),
            WebTarget::Tag(tag) => format!("{web_base}/{owner}/{repo}/-/tags/{tag}"),
        }
    }

    async fn get_login(&self, token: &str) -> Result<String> {
        // GitLab PAT auth via the PRIVATE-TOKEN header (`GET /user`).
        let resp = self
            .http
            .get(format!("{}/user", self.base_url))
            .header("PRIVATE-TOKEN", token)
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
        let user: GitLabUser = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(user.username)
    }

    async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String> {
        // GitLab calls these "merge requests". The project id is the
        // URL-encoded `namespace/project` path.
        let project = format!("{}%2F{}", slug.owner, slug.repo);
        let url = format!("{}/projects/{}/merge_requests", self.base_url, project);
        // GitLab marks drafts via a `Draft:` title prefix.
        let title = if pr.draft {
            format!("Draft: {}", pr.title)
        } else {
            pr.title.clone()
        };
        let resp = self
            .http
            .post(&url)
            .header("PRIVATE-TOKEN", token)
            .json(&serde_json::json!({
                "source_branch": pr.head,
                "target_branch": pr.base,
                "title": title,
                "description": pr.body,
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
        let body: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        body.get("web_url")
            .and_then(|u| u.as_str())
            .map(String::from)
            .ok_or_else(|| Error::Http("MR response missing web_url".to_string()))
    }

    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo> {
        // POST /projects creates under the authenticated user's namespace.
        let resp = self
            .http
            .post(format!("{}/projects", self.base_url))
            .header("PRIVATE-TOKEN", token)
            .json(&serde_json::json!({
                "name": repo.name,
                "visibility": if repo.private { "private" } else { "public" },
                "description": repo.description,
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
        let body: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(RepoInfo {
            clone_url: body
                .get("http_url_to_repo")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            web_url: body
                .get("web_url")
                .and_then(|v| v.as_str())
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

    #[test]
    fn detects_gitlab_remote() {
        let c = GitLabClient::new();
        let remotes = vec![
            "https://github.com/a/b.git".to_string(),
            "git@gitlab.com:group/proj.git".to_string(),
        ];
        let slug = c.detect_slug(&remotes).expect("detect gitlab");
        assert_eq!(slug.owner, "group");
        assert_eq!(slug.repo, "proj");
    }

    #[tokio::test]
    async fn get_login_returns_username() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "username": "ada"
            })))
            .mount(&server)
            .await;
        let c = GitLabClient::with_base_url(server.uri());
        assert_eq!(c.get_login("tok").await.expect("login"), "ada");
    }

    #[tokio::test]
    async fn create_merge_request_returns_url_and_surfaces_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/projects/group%2Fproj/merge_requests"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "web_url": "https://gitlab.com/group/proj/-/merge_requests/7"
            })))
            .mount(&server)
            .await;
        let c = GitLabClient::with_base_url(server.uri());
        let pr = NewPullRequest {
            head: "feature".into(),
            base: "main".into(),
            title: "Add".into(),
            body: "b".into(),
            draft: false,
        };
        let url = c
            .create_pull_request("tok", &RepoSlug::new("group", "proj"), &pr)
            .await
            .expect("create MR");
        assert_eq!(url, "https://gitlab.com/group/proj/-/merge_requests/7");

        let server409 = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/projects/group%2Fproj/merge_requests"))
            .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
                "message": ["Another open merge request already exists"]
            })))
            .mount(&server409)
            .await;
        let c409 = GitLabClient::with_base_url(server409.uri());
        let err = c409
            .create_pull_request("tok", &RepoSlug::new("group", "proj"), &pr)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Api { status: 409, .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn create_repo_returns_urls() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/projects"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "http_url_to_repo": "https://gitlab.com/ada/newproj.git",
                "web_url": "https://gitlab.com/ada/newproj"
            })))
            .mount(&server)
            .await;
        let c = GitLabClient::with_base_url(server.uri());
        let info = c
            .create_repo(
                "tok",
                &NewRepo {
                    name: "newproj".into(),
                    private: false,
                    description: "d".into(),
                    owner: None,
                    project: None,
                },
            )
            .await
            .expect("create repo");
        assert_eq!(info.clone_url, "https://gitlab.com/ada/newproj.git");
        assert_eq!(info.web_url, "https://gitlab.com/ada/newproj");
    }
}
