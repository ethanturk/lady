//! GitHub provider (PH3-011/012 + PH4-005 create_repo).

use serde::Deserialize;

use crate::{
    api_error_message, detect_github_slug, Error, ForgeKind, HostingProvider, NewPullRequest,
    NewRepo, Notification, RepoInfo, RepoSlug, Result,
};

/// Convert a notification subject's API URL to a best-effort browser URL.
fn notification_html_url(api_url: &str, full_name: &str) -> String {
    if let Some(rest) = api_url.strip_prefix("https://api.github.com/repos/") {
        // `.../pulls/N` → `.../pull/N`; issues/commits map through unchanged.
        return format!(
            "https://github.com/{}",
            rest.replacen("/pulls/", "/pull/", 1)
        );
    }
    format!("https://github.com/{full_name}")
}

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

    /// List the authenticated user's notification threads (PH4-006).
    pub async fn list_notifications(&self, token: &str) -> Result<Vec<Notification>> {
        let resp = self
            .http
            .get(format!("{}/notifications", self.base_url))
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
        let arr: Vec<serde_json::Value> =
            resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(arr
            .iter()
            .map(|n| {
                let repo = n
                    .pointer("/repository/full_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let api_url = n
                    .pointer("/subject/url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                Notification {
                    id: n
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    title: n
                        .pointer("/subject/title")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    kind: n
                        .pointer("/subject/type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    url: notification_html_url(api_url, &repo),
                    repo,
                    unread: n.get("unread").and_then(|v| v.as_bool()).unwrap_or(false),
                    updated: n
                        .get("updated_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }
            })
            .collect())
    }

    /// Mark a notification thread read (PH4-006).
    pub async fn mark_read(&self, token: &str, id: &str) -> Result<()> {
        let resp = self
            .http
            .patch(format!("{}/notifications/threads/{}", self.base_url, id))
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
        Ok(())
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
        // POST /orgs/{org}/repos when an owner org is given, else /user/repos
        // under the authenticated user (PH4-005).
        let url = match repo.owner.as_deref() {
            Some(org) if !org.is_empty() => format!("{}/orgs/{}/repos", self.base_url, org),
            _ => format!("{}/user/repos", self.base_url),
        };
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

    #[tokio::test]
    async fn create_repo_under_user_returns_urls() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/user/repos"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "clone_url": "https://github.com/octocat/newrepo.git",
                "html_url": "https://github.com/octocat/newrepo"
            })))
            .mount(&server)
            .await;
        let c = GitHubClient::with_base_url(server.uri());
        let info = c
            .create_repo(
                "tok",
                &NewRepo {
                    name: "newrepo".into(),
                    private: true,
                    description: "d".into(),
                    owner: None,
                    project: None,
                },
            )
            .await
            .expect("create repo");
        assert_eq!(info.clone_url, "https://github.com/octocat/newrepo.git");
        assert_eq!(info.web_url, "https://github.com/octocat/newrepo");
    }

    #[tokio::test]
    async fn list_notifications_and_mark_read() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/notifications"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "id": "123",
                    "unread": true,
                    "updated_at": "2026-06-01T10:00:00Z",
                    "subject": {
                        "title": "Fix the bug",
                        "type": "PullRequest",
                        "url": "https://api.github.com/repos/octocat/hello/pulls/7"
                    },
                    "repository": { "full_name": "octocat/hello" }
                }
            ])))
            .mount(&server)
            .await;
        let c = GitHubClient::with_base_url(server.uri());
        let notes = c.list_notifications("tok").await.expect("list");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, "123");
        assert_eq!(notes[0].title, "Fix the bug");
        assert_eq!(notes[0].repo, "octocat/hello");
        assert_eq!(notes[0].kind, "PullRequest");
        assert!(notes[0].unread);
        // PR API url is converted to a browser /pull/ url.
        assert_eq!(notes[0].url, "https://github.com/octocat/hello/pull/7");

        let server2 = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/notifications/threads/123"))
            .respond_with(ResponseTemplate::new(205))
            .mount(&server2)
            .await;
        let c2 = GitHubClient::with_base_url(server2.uri());
        c2.mark_read("tok", "123").await.expect("mark read");
    }
}
