//! Azure OpenAI provider (PH5-004). Deployment-scoped Chat Completions:
//! the model is the deployment in the URL, auth is the `api-key` header.

use crate::providers::{http_client, openai_chat};
use crate::{AiProvider, AiRequest, AiResponse, Result, StreamSink};

/// Azure OpenAI API version pinned for the chat completions shape.
const API_VERSION: &str = "2024-06-01";

/// An Azure OpenAI client. `endpoint` is the resource base
/// (`https://<resource>.openai.azure.com`); `deployment` is the model.
pub struct AzureOpenAiProvider {
    endpoint: String,
    deployment: String,
    api_key: String,
    http: reqwest::Client,
}

impl AzureOpenAiProvider {
    /// Build a client for `endpoint` + `deployment`.
    pub fn new(endpoint: &str, deployment: &str, api_key: impl Into<String>) -> Self {
        AzureOpenAiProvider {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            deployment: deployment.to_string(),
            api_key: api_key.into(),
            http: http_client(),
        }
    }
}

#[async_trait::async_trait]
impl AiProvider for AzureOpenAiProvider {
    fn id(&self) -> &str {
        "azure-openai"
    }
    fn context_window(&self) -> usize {
        128_000
    }
    fn is_remote(&self) -> bool {
        true
    }
    async fn complete(&self, req: &AiRequest, sink: &mut StreamSink<'_>) -> Result<AiResponse> {
        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.endpoint, self.deployment, API_VERSION
        );
        let builder = self.http.post(url).header("api-key", &self.api_key);
        // Azure scopes the model in the URL, so omit it from the body.
        openai_chat(builder, req, false, sink).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AiTask, CancelToken};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn streams_deployment_scoped() {
        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
                   data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/openai/deployments/my-dep/chat/completions"))
            .and(header("api-key", "azkey"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let p = AzureOpenAiProvider::new(&server.uri(), "my-dep", "azkey");
        let mut buf = String::new();
        let mut cb = |d: &str| buf.push_str(d);
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(&AiRequest::new(AiTask::CommitMessage, "ignored"), &mut sink)
            .await
            .expect("complete");
        assert_eq!(resp.text, "ok");
    }
}
