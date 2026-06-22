//! Mistral Chat Completions provider (PH5-004). OpenAI-compatible shape.

use crate::providers::{http_client, openai_chat, TemperatureParam, TokenLimitParam};
use crate::{AiProvider, AiRequest, AiResponse, Result, StreamSink};

/// A Mistral API client (bearer auth, OpenAI-style chat body).
pub struct MistralProvider {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl MistralProvider {
    /// Build a client for the public Mistral API.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url("https://api.mistral.ai/v1", api_key)
    }

    /// Build a client against a custom base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        MistralProvider {
            base_url: base_url.into(),
            api_key: api_key.into(),
            http: http_client(),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for MistralProvider {
    fn id(&self) -> &str {
        "mistral"
    }
    fn context_window(&self) -> usize {
        32_000
    }
    fn is_remote(&self) -> bool {
        true
    }
    async fn complete(&self, req: &AiRequest, sink: &mut StreamSink<'_>) -> Result<AiResponse> {
        let builder = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key);
        openai_chat(
            builder,
            req,
            true,
            TokenLimitParam::MaxTokens,
            TemperatureParam::Include,
            sink,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AiTask, CancelToken};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn streams_sse_deltas() {
        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n\
                   data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = MistralProvider::with_base_url(server.uri(), "key");
        let mut buf = String::new();
        let mut cb = |d: &str| buf.push_str(d);
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(
                &AiRequest::new(AiTask::Explain, "mistral-small-latest"),
                &mut sink,
            )
            .await
            .expect("complete");
        assert_eq!(resp.text, "hello");
    }
}
