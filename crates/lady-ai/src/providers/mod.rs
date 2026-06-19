//! Concrete [`AiProvider`](crate::AiProvider) implementations — one thin
//! `reqwest`/rustls client per provider (ADR-0011). Streaming is hand-rolled
//! over the HTTP body so we control budgeting + cancellation directly.

use crate::{AiRequest, AiResponse, Error, Result, StreamSink};

pub mod anthropic;
pub mod azure;
pub mod gemini;
pub mod mistral;
pub mod openai;

/// A `reqwest` client with Lady's user-agent.
pub(crate) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Lady")
        .build()
        .expect("build reqwest client")
}

/// Map a non-success response to [`Error::Api`], reading the body for a message.
pub(crate) async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(Error::Api {
        status: status.as_u16(),
        message: api_error_message(&body),
    })
}

/// Best-effort error message from a JSON error body across provider shapes.
fn api_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            // OpenAI/Azure/Mistral: { "error": { "message": ... } }
            v.pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(String::from)
                // Anthropic: { "error": { "message": ... } } (same) or { "message" }
                .or_else(|| v.get("message").and_then(|m| m.as_str()).map(String::from))
                // Gemini: { "error": { "message" } } handled above; fallback string error.
                .or_else(|| v.get("error").and_then(|m| m.as_str()).map(String::from))
        })
        .unwrap_or_else(|| body.to_string())
        .chars()
        .take(500)
        .collect()
}

/// Run an OpenAI-style Chat Completions streaming request. `builder` is an
/// already-authed POST to the chat endpoint; `include_model` controls whether
/// the model id goes in the body (false for Azure, which scopes it in the URL).
/// Shared by OpenAI, Azure OpenAI, and Mistral.
pub(crate) async fn openai_chat(
    builder: reqwest::RequestBuilder,
    req: &AiRequest,
    include_model: bool,
    sink: &mut StreamSink<'_>,
) -> Result<AiResponse> {
    let mut body = serde_json::json!({
        "stream": true,
        "temperature": req.temperature,
        "max_tokens": req.max_tokens,
        "messages": [
            { "role": "system", "content": req.system },
            { "role": "user", "content": req.prompt },
        ],
    });
    if include_model {
        body["model"] = serde_json::Value::String(req.model.clone());
    }
    let resp = builder
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::Http(e.to_string()))?;
    let resp = check_status(resp).await?;
    stream_sse(resp, sink, |v| {
        v.pointer("/choices/0/delta/content")
            .and_then(|c| c.as_str())
            .map(String::from)
    })
    .await
}

/// Stream a Server-Sent-Events body, extracting a token delta from each `data:`
/// JSON payload via `extract`. Stops on a `[DONE]` sentinel or end of body.
/// `extract` returning `None` (e.g. a non-text event) is skipped.
pub(crate) async fn stream_sse<F>(
    resp: reqwest::Response,
    sink: &mut StreamSink<'_>,
    mut extract: F,
) -> Result<AiResponse>
where
    F: FnMut(&serde_json::Value) -> Option<String> + Send,
{
    stream_body(resp, sink, true, &mut extract).await
}

async fn stream_body(
    mut resp: reqwest::Response,
    sink: &mut StreamSink<'_>,
    sse: bool,
    extract: &mut (dyn FnMut(&serde_json::Value) -> Option<String> + Send),
) -> Result<AiResponse> {
    let mut buf = String::new();
    let mut text = String::new();
    loop {
        let chunk = resp.chunk().await.map_err(|e| Error::Http(e.to_string()))?;
        let Some(bytes) = chunk else { break };
        buf.push_str(&String::from_utf8_lossy(&bytes));
        // Process complete lines; keep any trailing partial line in `buf`.
        while let Some(nl) = buf.find('\n') {
            let line: String = buf.drain(..=nl).collect();
            let line = line.trim_end();
            let payload = if sse {
                let Some(rest) = line.strip_prefix("data:") else {
                    continue; // ignore `event:`/comment/blank lines
                };
                rest.trim()
            } else {
                if line.is_empty() {
                    continue;
                }
                line
            };
            if payload.is_empty() {
                continue;
            }
            if sse && payload == "[DONE]" {
                return Ok(AiResponse { text });
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(delta) = extract(&v) {
                    if !delta.is_empty() {
                        sink.push(&delta)?;
                        text.push_str(&delta);
                    }
                }
            }
        }
    }
    Ok(AiResponse { text })
}
