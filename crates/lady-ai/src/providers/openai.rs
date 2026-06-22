//! OpenAI Chat Completions provider (PH5-004). Also backs any OpenAI-compatible
//! server (Ollama, LM Studio, vLLM, …) via [`OpenAiProvider::with_base_url`].

use crate::providers::{http_client, openai_chat, TemperatureParam, TokenLimitParam};
use crate::{AiProvider, AiRequest, AiResponse, Error, Result, StreamSink};

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

    /// Build a client against a custom base URL (any OpenAI-compatible server).
    pub fn with_base_url(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        OpenAiProvider {
            base_url: base_url.into(),
            api_key: api_key.into(),
            http: http_client(),
        }
    }

    /// Attach bearer auth only when a key is set (local servers need none).
    fn auth(&self, b: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.is_empty() {
            b
        } else {
            b.bearer_auth(&self.api_key)
        }
    }

    /// List model ids from the server's `/models` endpoint (OpenAI-compatible).
    /// Surfaces a friendly [`Error::Unavailable`] when the server is unreachable.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let resp = self
            .auth(self.http.get(format!("{}/models", self.base_url)))
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    Error::Unavailable(
                        "The OpenAI-compatible server is not reachable — check the base URL and that it is running."
                            .to_string(),
                    )
                } else {
                    Error::Http(e.to_string())
                }
            })?;
        let resp = crate::providers::check_status(resp).await?;
        let v: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(v.get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn uses_max_completion_tokens(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.starts_with("gpt-5")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
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
        let builder = self.auth(
            self.http
                .post(format!("{}/chat/completions", self.base_url)),
        );
        let token_limit = if uses_max_completion_tokens(&req.model) {
            TokenLimitParam::MaxCompletionTokens
        } else {
            TokenLimitParam::MaxTokens
        };
        let temperature = if uses_max_completion_tokens(&req.model) {
            TemperatureParam::Omit
        } else {
            TemperatureParam::Include
        };
        openai_chat(builder, req, true, token_limit, temperature, sink).await
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
    async fn gpt5_uses_max_completion_tokens() {
        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"feat: ok\"}}]}\n\n\
                   data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = OpenAiProvider::with_base_url(server.uri(), "sk-test");
        let mut cb = |_d: &str| {};
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let mut req = AiRequest::new(AiTask::CommitMessage, "gpt-5");
        req.max_tokens = 321;

        p.complete(&req, &mut sink).await.expect("complete");

        let requests = server.received_requests().await.expect("request log");
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("json body");
        assert_eq!(body["max_completion_tokens"], 321);
        assert!(body.get("max_tokens").is_none(), "body: {body}");
        assert!(body.get("temperature").is_none(), "body: {body}");
    }

    #[tokio::test]
    async fn skips_reasoning_and_keeps_answer() {
        // Reasoning models stream `delta.reasoning_content` (thinking) before the
        // real `delta.content` answer. Only the answer must end up in the result,
        // and the reasoning must not be streamed to the sink.
        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking…\"}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"feat: x\"}}]}\n\n\
                   data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = OpenAiProvider::with_base_url(server.uri(), "");
        let mut buf = String::new();
        let mut cb = |d: &str| buf.push_str(d);
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(&AiRequest::new(AiTask::CommitMessage, "gemma"), &mut sink)
            .await
            .expect("complete");
        assert_eq!(resp.text, "feat: x");
        assert_eq!(buf, "feat: x", "reasoning must not be streamed to the sink");
    }

    #[tokio::test]
    async fn cancel_stops_even_with_no_answer_deltas() {
        // A reasoning model emits no answer deltas while thinking, so `push` (and
        // its cancel check) never runs. The per-chunk guard must still honor a
        // cancel — here pre-cancelled, so complete bails with Cancelled rather
        // than draining the reasoning-only stream.
        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\"}}]}\n\n\
                   data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = OpenAiProvider::with_base_url(server.uri(), "");
        let mut cb = |_d: &str| {};
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut sink = StreamSink::new(&mut cb, cancel);
        let err = p
            .complete(&AiRequest::new(AiTask::CommitMessage, "gemma"), &mut sink)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::Error::Cancelled), "got {err:?}");
    }

    #[tokio::test]
    async fn errors_when_only_reasoning_no_answer() {
        // Budget-truncated mid-thought: reasoning but no content. Must fail loudly
        // (BadOutput) rather than silently resolving to an empty message.
        let server = MockServer::start().await;
        let sse =
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"still thinking\"}}]}\n\n\
                   data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = OpenAiProvider::with_base_url(server.uri(), "");
        let mut cb = |_d: &str| {};
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let err = p
            .complete(&AiRequest::new(AiTask::CommitMessage, "gemma"), &mut sink)
            .await
            .unwrap_err();
        assert!(matches!(err, crate::Error::BadOutput(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn lists_models_from_compatible_server() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [ { "id": "llama3.1" }, { "id": "qwen2.5-coder" } ]
            })))
            .mount(&server)
            .await;
        // Empty key → no Authorization header sent (local-server friendly).
        let p = OpenAiProvider::with_base_url(server.uri(), "");
        let models = p.list_models().await.expect("models");
        assert_eq!(models, vec!["llama3.1", "qwen2.5-coder"]);
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
