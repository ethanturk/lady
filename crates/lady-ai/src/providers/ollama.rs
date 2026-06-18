//! Local Ollama provider (PH5-003) — the "never leaves your machine" path
//! (ADR-0008/0009). No consent gate; talks to a local HTTP endpoint.

use crate::providers::{http_client, stream_ndjson};
use crate::{AiProvider, AiRequest, AiResponse, Error, Result, StreamSink};

/// A client for a local Ollama server (default `http://localhost:11434`).
pub struct OllamaProvider {
    base_url: String,
    http: reqwest::Client,
}

impl OllamaProvider {
    /// Build a provider against `host` (the Ollama base URL).
    pub fn new(host: &str) -> Self {
        OllamaProvider {
            base_url: host.trim_end_matches('/').to_string(),
            http: http_client(),
        }
    }

    /// List locally available model names (`/api/tags`). Surfaces a friendly
    /// [`Error::Unavailable`] when Ollama is not running.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let resp = self
            .http
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map_err(unavailable)?;
        let v: serde_json::Value = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(v.get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// Connection refused / DNS / timeout → a clear "not running" message.
fn unavailable(e: reqwest::Error) -> Error {
    if e.is_connect() || e.is_timeout() {
        Error::Unavailable(
            "Ollama is not reachable — install it and run `ollama serve`, then pull a model."
                .to_string(),
        )
    } else {
        Error::Http(e.to_string())
    }
}

#[async_trait::async_trait]
impl AiProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }
    fn context_window(&self) -> usize {
        // Conservative; most local models are 8k+ but we budget low to be safe.
        8192
    }
    fn is_remote(&self) -> bool {
        false
    }

    async fn complete(&self, req: &AiRequest, sink: &mut StreamSink<'_>) -> Result<AiResponse> {
        let body = serde_json::json!({
            "model": req.model,
            "stream": true,
            "options": { "temperature": req.temperature },
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.prompt },
            ],
        });
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(unavailable)?;
        let resp = crate::providers::check_status(resp).await?;
        // Each ndjson line: { "message": { "content": "..." }, "done": bool }.
        stream_ndjson(resp, sink, |v| {
            v.pointer("/message/content")
                .and_then(|c| c.as_str())
                .map(String::from)
        })
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
    async fn streams_chat_ndjson() {
        let server = MockServer::start().await;
        let ndjson = "{\"message\":{\"content\":\"feat: \"},\"done\":false}\n\
                      {\"message\":{\"content\":\"add\"},\"done\":true}\n";
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ndjson))
            .mount(&server)
            .await;
        let p = OllamaProvider::new(&server.uri());
        let mut buf = String::new();
        let mut cb = |d: &str| buf.push_str(d);
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(
                &AiRequest::new(AiTask::CommitMessage, "llama3.1"),
                &mut sink,
            )
            .await
            .expect("complete");
        assert_eq!(resp.text, "feat: add");
        assert_eq!(buf, "feat: add");
    }

    #[tokio::test]
    async fn lists_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [ { "name": "llama3.1" }, { "name": "qwen2.5-coder" } ]
            })))
            .mount(&server)
            .await;
        let p = OllamaProvider::new(&server.uri());
        let models = p.list_models().await.expect("models");
        assert_eq!(models, vec!["llama3.1", "qwen2.5-coder"]);
    }

    #[tokio::test]
    async fn unreachable_is_friendly() {
        // Nothing listening on this port → connect error → Unavailable.
        let p = OllamaProvider::new("http://127.0.0.1:1");
        let err = p.list_models().await.unwrap_err();
        assert!(matches!(err, Error::Unavailable(_)), "got {err:?}");
    }
}
