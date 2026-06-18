//! OpenAI Chat Completions provider (PH5-004).

use crate::providers::{http_client, openai_chat};
use crate::{AiProvider, AiRequest, AiResponse, Result, StreamSink};

/// An OpenAI API client (bearer auth, model in body).
pub struct OpenAiProvider {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl OpenAiProvider {
    /// Build a client for the public OpenAI API.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url("https://api.openai.com/v1", api_key)
    }

    /// Build a client against a custom base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        OpenAiProvider {
            base_url: base_url.into(),
            api_key: api_key.into(),
            http: http_client(),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for OpenAiProvider {
    fn id(&self) -> &str {
        "openai"
    }
    fn context_window(&self) -> usize {
        128_000
    }
    fn is_remote(&self) -> bool {
        true
    }
    async fn complete(&self, req: &AiRequest, sink: &mut StreamSink<'_>) -> Result<AiResponse> {
        let builder = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key);
        openai_chat(builder, req, true, sink).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AiTask, CancelToken};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn streams_sse_deltas() {
        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"feat: \"}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"add\"}}]}\n\n\
                   data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = OpenAiProvider::with_base_url(server.uri(), "sk-test");
        let mut buf = String::new();
        let mut cb = |d: &str| buf.push_str(d);
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(
                &AiRequest::new(AiTask::CommitMessage, "gpt-4o-mini"),
                &mut sink,
            )
            .await
            .expect("complete");
        assert_eq!(resp.text, "feat: add");
    }

    #[tokio::test]
    async fn surfaces_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": { "message": "Incorrect API key provided" }
            })))
            .mount(&server)
            .await;
        let p = OpenAiProvider::with_base_url(server.uri(), "bad");
        let mut cb = |_d: &str| {};
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let err = p
            .complete(
                &AiRequest::new(AiTask::CommitMessage, "gpt-4o-mini"),
                &mut sink,
            )
            .await
            .unwrap_err();
        match err {
            crate::Error::Api { status, message } => {
                assert_eq!(status, 401);
                assert!(message.contains("Incorrect API key"), "msg: {message}");
            }
            other => panic!("expected Api error, got {other:?}"),
        }
    }
}
