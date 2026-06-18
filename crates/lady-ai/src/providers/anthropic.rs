//! Anthropic Claude Messages provider (PH5-004). SSE with typed events;
//! text arrives in `content_block_delta` events as `delta.text`.

use crate::providers::{check_status, http_client, stream_sse};
use crate::{AiProvider, AiRequest, AiResponse, Error, Result, StreamSink};

/// Anthropic API version header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// An Anthropic API client (`x-api-key` auth, top-level `system`).
pub struct AnthropicProvider {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl AnthropicProvider {
    /// Build a client for the public Anthropic API.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url("https://api.anthropic.com/v1", api_key)
    }

    /// Build a client against a custom base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        AnthropicProvider {
            base_url: base_url.into(),
            api_key: api_key.into(),
            http: http_client(),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }
    fn context_window(&self) -> usize {
        200_000
    }
    fn is_remote(&self) -> bool {
        true
    }
    async fn complete(&self, req: &AiRequest, sink: &mut StreamSink<'_>) -> Result<AiResponse> {
        let body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "stream": true,
            "system": req.system,
            "messages": [ { "role": "user", "content": req.prompt } ],
        });
        let resp = self
            .http
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let resp = check_status(resp).await?;
        stream_sse(resp, sink, |v| {
            // content_block_delta → { "delta": { "type": "text_delta", "text": ... } }
            v.pointer("/delta/text")
                .and_then(|t| t.as_str())
                .map(String::from)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AiTask, CancelToken};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn streams_content_block_deltas() {
        let server = MockServer::start().await;
        let sse = "event: message_start\ndata: {\"type\":\"message_start\"}\n\n\
                   event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Add \"}}\n\n\
                   event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"widget\"}}\n\n\
                   event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        Mock::given(method("POST"))
            .and(path("/messages"))
            .and(header("x-api-key", "ak"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = AnthropicProvider::with_base_url(server.uri(), "ak");
        let mut buf = String::new();
        let mut cb = |d: &str| buf.push_str(d);
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(
                &AiRequest::new(AiTask::Explain, "claude-3-5-sonnet-latest"),
                &mut sink,
            )
            .await
            .expect("complete");
        assert_eq!(resp.text, "Add widget");
    }
}
