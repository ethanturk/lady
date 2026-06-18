//! Bitbucket provider. Detection + resolution land in PH4-001; the API calls
//! (pull request, repo create) are implemented in PH4-003 / PH4-005.

use crate::{
    api_error_message, owner_repo_slug, remote_host, Error, ForgeKind, HostingProvider,
    NewPullRequest, NewRepo, RepoInfo, RepoSlug, Result,
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
        // Bearer auth with an OAuth / access token (`GET /user`).
        let resp = self
            .http
            .get(format!("{}/user", self.base_url))
            .bearer_auth(token)
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
        let body: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        // Bitbucket prefers `nickname`; fall back to `username` / `account_id`.
        for key in ["nickname", "username", "account_id"] {
            if let Some(v) = body.get(key).and_then(|v| v.as_str()) {
                return Ok(v.to_string());
            }
        }
        Ok("bitbucket".to_string())
    }

    async fn create_pull_request(
        &self,
        token: &str,
        slug: &RepoSlug,
        pr: &NewPullRequest,
    ) -> Result<String> {
        // POST /repositories/{workspace}/{repo}/pullrequests. Bitbucket Cloud
        // has no draft flag, so `draft` is ignored.
        let url = format!(
            "{}/repositories/{}/{}/pullrequests",
            self.base_url, slug.owner, slug.repo
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&serde_json::json!({
                "title": pr.title,
                "description": pr.body,
                "source": { "branch": { "name": pr.head } },
                "destination": { "branch": { "name": pr.base } },
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
        // The browser URL is `links.html.href`.
        body.get("links")
            .and_then(|l| l.get("html"))
            .and_then(|h| h.get("href"))
            .and_then(|u| u.as_str())
            .map(String::from)
            .ok_or_else(|| Error::Http("PR response missing links.html.href".to_string()))
    }

    async fn create_repo(&self, token: &str, repo: &NewRepo) -> Result<RepoInfo> {
        // Bitbucket creates under a workspace: POST /repositories/{ws}/{slug}.
        let workspace = repo
            .owner
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::Api {
                status: 0,
                message: "Bitbucket needs a workspace (owner) to create a repo".to_string(),
            })?;
        let url = format!("{}/repositories/{}/{}", self.base_url, workspace, repo.name);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&serde_json::json!({
                "scm": "git",
                "is_private": repo.private,
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
        // The https clone link lives in `links.clone[] { name: "https", href }`.
        let clone_url = body
            .get("links")
            .and_then(|l| l.get("clone"))
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("https"))
                    .or_else(|| arr.first())
            })
            .and_then(|e| e.get("href"))
            .and_then(|h| h.as_str())
            .unwrap_or_default()
            .to_string();
        let web_url = body
            .get("links")
            .and_then(|l| l.get("html"))
            .and_then(|h| h.get("href"))
            .and_then(|u| u.as_str())
            .unwrap_or_default()
            .to_string();
        Ok(RepoInfo { clone_url, web_url })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn detects_bitbucket_remote() {
        let c = BitbucketClient::new();
        let remotes = vec!["git@bitbucket.org:team/proj.git".to_string()];
        let slug = c.detect_slug(&remotes).expect("detect bitbucket");
        assert_eq!(slug.owner, "team");
        assert_eq!(slug.repo, "proj");
    }

    #[tokio::test]
    async fn get_login_returns_nickname() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "nickname": "bbuser"
            })))
            .mount(&server)
            .await;
        let c = BitbucketClient::with_base_url(server.uri());
        assert_eq!(c.get_login("tok").await.expect("login"), "bbuser");
    }

    #[tokio::test]
    async fn create_pull_request_returns_url_and_surfaces_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repositories/team/proj/pullrequests"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "links": { "html": { "href": "https://bitbucket.org/team/proj/pull-requests/3" } }
            })))
            .mount(&server)
            .await;
        let c = BitbucketClient::with_base_url(server.uri());
        let pr = NewPullRequest {
            head: "feature".into(),
            base: "main".into(),
            title: "Add".into(),
            body: "b".into(),
            draft: false,
        };
        let url = c
            .create_pull_request("tok", &RepoSlug::new("team", "proj"), &pr)
            .await
            .expect("create PR");
        assert_eq!(url, "https://bitbucket.org/team/proj/pull-requests/3");

        let server400 = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repositories/team/proj/pullrequests"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": { "message": "A pull request already exists." }
            })))
            .mount(&server400)
            .await;
        let c400 = BitbucketClient::with_base_url(server400.uri());
        match c400
            .create_pull_request("tok", &RepoSlug::new("team", "proj"), &pr)
            .await
            .unwrap_err()
        {
            Error::Api { status, message } => {
                assert_eq!(status, 400);
                assert!(message.contains("already exists"), "msg: {message}");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_repo_under_workspace_returns_urls() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repositories/team/newrepo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "links": {
                    "html": { "href": "https://bitbucket.org/team/newrepo" },
                    "clone": [
                        { "name": "https", "href": "https://bitbucket.org/team/newrepo.git" },
                        { "name": "ssh", "href": "git@bitbucket.org:team/newrepo.git" }
                    ]
                }
            })))
            .mount(&server)
            .await;
        let c = BitbucketClient::with_base_url(server.uri());
        let info = c
            .create_repo(
                "tok",
                &NewRepo {
                    name: "newrepo".into(),
                    private: true,
                    description: "d".into(),
                    owner: Some("team".into()),
                    project: None,
                },
            )
            .await
            .expect("create repo");
        assert_eq!(info.clone_url, "https://bitbucket.org/team/newrepo.git");
        assert_eq!(info.web_url, "https://bitbucket.org/team/newrepo");
    }
}
