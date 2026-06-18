//! GitHub provider (PH3-011/012 + PH4-005 create_repo).

use serde::Deserialize;

use crate::{
    api_error_message, detect_github_slug, Error, ForgeKind, HostingProvider, NewPullRequest,
    NewRepo, RepoInfo, RepoSlug, Result,
};

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
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

    /// A client against a custom base URL (tests / GitHub Enterprise).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        GitHubClient {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent("Lady")
                .build()
                .expect("build reqwest client"),
        }
    }
}

#[async_trait::async_trait]
impl HostingProvider for GitHubClient {
    fn kind(&self) -> ForgeKind {
        ForgeKind::GitHub
    }

    fn detect_slug(&self, remote_urls: &[String]) -> Option<RepoSlug> {
        detect_github_slug(remote_urls)
    }

    async fn get_login(&self, token: &str) -> Result<String> {
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
        let user: GitHubUser = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(user.login)
    }

    async fn create_pull_request(
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
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status: status.as_u16(),
                message: api_error_message(&body),
            });
        }
        let body: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        body.get("html_url")
            .and_then(|u| u.as_str())
            .map(String::from)
            .ok_or_else(|| Error::Http("PR response missing html_url".to_string()))
    }

    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo> {
        // POST /user/repos creates under the authenticated user (PH4-005).
        let url = format!("{}/user/repos", self.base_url);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::json!({
                "name": repo.name,
                "private": repo.private,
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
                .get("clone_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            web_url: body
                .get("html_url")
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
    fn detects_github_among_remotes() {
        let c = GitHubClient::new();
        let remotes = vec![
            "https://gitlab.com/a/b.git".to_string(),
            "git@github.com:owner/repo.git".to_string(),
        ];
        let slug = c.detect_slug(&remotes).expect("detect github");
        assert_eq!(slug.owner, "owner");
        assert_eq!(slug.repo, "repo");
    }

    #[tokio::test]
    async fn get_login_returns_login_on_200_and_unauthorized_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "octocat"
            })))
            .mount(&server)
            .await;
        let client = GitHubClient::with_base_url(server.uri());
        assert_eq!(client.get_login("good").await.expect("login"), "octocat");

        let server401 = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server401)
            .await;
        let c401 = GitHubClient::with_base_url(server401.uri());
        assert!(matches!(
            c401.get_login("bad").await.unwrap_err(),
            Error::Unauthorized
        ));
    }

    #[tokio::test]
    async fn create_pull_request_returns_url_and_surfaces_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/octocat/hello/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "html_url": "https://github.com/octocat/hello/pull/42"
            })))
            .mount(&server)
            .await;
        let client = GitHubClient::with_base_url(server.uri());
        let pr = NewPullRequest {
            head: "feature".into(),
            base: "main".into(),
            title: "t".into(),
            body: "b".into(),
            draft: false,
        };
        let url = client
            .create_pull_request("tok", &RepoSlug::new("octocat", "hello"), &pr)
            .await
            .expect("create PR");
        assert_eq!(url, "https://github.com/octocat/hello/pull/42");

        let server422 = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/octocat/hello/pulls"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "A pull request already exists for octocat:feature."
            })))
            .mount(&server422)
            .await;
        let c422 = GitHubClient::with_base_url(server422.uri());
        match c422
            .create_pull_request("tok", &RepoSlug::new("octocat", "hello"), &pr)
            .await
            .unwrap_err()
        {
            Error::Api { status, message } => {
                assert_eq!(status, 422);
                assert!(message.contains("already exists"), "msg: {message}");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }
}
