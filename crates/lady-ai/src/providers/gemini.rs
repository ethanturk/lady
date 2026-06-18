//! Google Gemini provider (PH5-004). `streamGenerateContent?alt=sse`; the key
//! is a query parameter and text is in `candidates[0].content.parts[0].text`.

use crate::providers::{check_status, http_client, stream_sse};
use crate::{AiProvider, AiRequest, AiResponse, Error, Result, StreamSink};

/// A Google Gemini API client (key as a query param).
pub struct GeminiProvider {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl GeminiProvider {
    /// Build a client for the public Gemini API.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url("https://generativelanguage.googleapis.com/v1beta", api_key)
    }

    /// Build a client against a custom base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        GeminiProvider {
            base_url: base_url.into(),
            api_key: api_key.into(),
            http: http_client(),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for GeminiProvider {
    fn id(&self) -> &str {
        "gemini"
    }
    fn context_window(&self) -> usize {
        1_000_000
    }
    fn is_remote(&self) -> bool {
        true
    }
    async fn complete(&self, req: &AiRequest, sink: &mut StreamSink<'_>) -> Result<AiResponse> {
        let url = format!(
            "{}/models/{}:streamGenerateContent",
            self.base_url, req.model
        );
        let body = serde_json::json!({
            "systemInstruction": { "parts": [ { "text": req.system } ] },
            "contents": [ { "role": "user", "parts": [ { "text": req.prompt } ] } ],
            "generationConfig": {
                "temperature": req.temperature,
                "maxOutputTokens": req.max_tokens,
            },
        });
        let resp = self
            .http
            .post(url)
            .query(&[("alt", "sse"), ("key", &self.api_key)])
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let resp = check_status(resp).await?;
        stream_sse(resp, sink, |v| {
            v.pointer("/candidates/0/content/parts/0/text")
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
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn streams_candidate_parts() {
        let server = MockServer::start().await;
        let sse = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello \"}]}}]}\n\n\
                   data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"world\"}]}}]}\n\n";
        Mock::given(method("POST"))
            .and(path("/models/gemini-1.5-flash:streamGenerateContent"))
            .and(query_param("key", "gk"))
            .and(query_param("alt", "sse"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = GeminiProvider::with_base_url(server.uri(), "gk");
        let mut buf = String::new();
        let mut cb = |d: &str| buf.push_str(d);
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(
                &AiRequest::new(AiTask::Explain, "gemini-1.5-flash"),
                &mut sink,
            )
            .await
            .expect("complete");
        assert_eq!(resp.text, "Hello world");
    }
}
