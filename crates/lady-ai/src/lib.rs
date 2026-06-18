//! `lady-ai` — Lady's provider-agnostic AI layer (PLAN.md §5; ADR-0008/0009/0011).
//!
//! This crate owns:
//! - the [`AiProvider`] trait — a thin `reqwest`/rustls client shape implemented
//!   per provider (ADR-0011), with streaming + cooperative cancellation;
//! - the request/response/task model ([`AiRequest`], [`AiResponse`], [`AiTask`]);
//! - the provider registry + per-feature model selection config ([`AiConfig`]),
//!   serde-serializable so the host app persists it to settings;
//! - the context builder, token budgeting, and the MANDATORY secret-redaction
//!   pass ([`context`]) run before any *remote* send (ADR-0009).
//!
//! Keys and consent live in the host app (OS keychain via `lady-hosting`'s
//! `TokenStore`, consent recorded in settings) — this crate never reads the
//! keychain or makes a remote call without being handed a key by the caller.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub mod context;
pub mod prompts;
mod providers;

pub use providers::{
    anthropic::AnthropicProvider, azure::AzureOpenAiProvider, gemini::GeminiProvider,
    mistral::MistralProvider, ollama::OllamaProvider, openai::OpenAiProvider,
};

/// Errors surfaced by AI operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A network/HTTP error talking to the provider.
    #[error("http error: {0}")]
    Http(String),
    /// The provider returned an error payload.
    #[error("provider error ({status}): {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Best-effort error message from the body.
        message: String,
    },
    /// The local provider (Ollama) is unreachable / not running.
    #[error("local model unavailable: {0}")]
    Unavailable(String),
    /// The operation was cancelled cooperatively by the caller.
    #[error("operation cancelled")]
    Cancelled,
    /// A remote call was attempted without recorded consent (ADR-0009).
    #[error("AI consent required for {0} before code can leave this machine")]
    ConsentRequired(String),
    /// The model returned output that did not match the expected structure.
    #[error("invalid model output: {0}")]
    BadOutput(String),
    /// No API key is configured for the selected remote provider.
    #[error("no API key configured for {0}")]
    NoKey(String),
}

/// Result alias for AI operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The AI tasks Lady drives (GitKraken parity + superset). Used to pick a model
/// per feature and to shape prompts in [`context`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AiTask {
    /// Generate a commit message for staged/selected changes (PH5-006).
    CommitMessage,
    /// Split a working tree into logical commits (PH5-007).
    SplitCommits,
    /// Explain a commit / branch range / stash / working changes (PH5-008).
    Explain,
    /// Propose a resolution for a merge-conflict region (PH5-009).
    ResolveConflict,
    /// Generate a pull/merge request title (PH5-010).
    PrTitle,
    /// Generate a pull/merge request description (PH5-010).
    PrDescription,
    /// Generate a changelog from a commit range (PH5-010).
    Changelog,
    /// Summarize working changes into a stash note (PH5-010).
    StashNote,
}

impl AiTask {
    /// A stable string id (used as the per-task model config key).
    pub fn id(self) -> &'static str {
        match self {
            AiTask::CommitMessage => "commit_message",
            AiTask::SplitCommits => "split_commits",
            AiTask::Explain => "explain",
            AiTask::ResolveConflict => "resolve_conflict",
            AiTask::PrTitle => "pr_title",
            AiTask::PrDescription => "pr_description",
            AiTask::Changelog => "changelog",
            AiTask::StashNote => "stash_note",
        }
    }
}

/// Which provider backs a request. `Ollama` is local-first (no consent gate);
/// the rest are remote and require consent + redaction (ADR-0009).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum ProviderKind {
    /// Local Ollama endpoint — never leaves the machine (ADR-0008/0009).
    Ollama,
    /// OpenAI Chat Completions.
    OpenAi,
    /// Anthropic Claude Messages.
    Anthropic,
    /// Google Gemini `generateContent`.
    Gemini,
    /// Azure OpenAI (deployment-scoped Chat Completions).
    AzureOpenAi,
    /// Mistral Chat Completions.
    Mistral,
}

impl ProviderKind {
    /// Every provider kind, for UI enumeration.
    pub const ALL: [ProviderKind; 6] = [
        ProviderKind::Ollama,
        ProviderKind::OpenAi,
        ProviderKind::Anthropic,
        ProviderKind::Gemini,
        ProviderKind::AzureOpenAi,
        ProviderKind::Mistral,
    ];

    /// Human-facing label.
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Ollama => "Ollama (local)",
            ProviderKind::OpenAi => "OpenAI",
            ProviderKind::Anthropic => "Anthropic Claude",
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::AzureOpenAi => "Azure OpenAI",
            ProviderKind::Mistral => "Mistral",
        }
    }

    /// Whether using this provider sends data off the machine. Local Ollama is
    /// the only `false` — it is the consent-free, redaction-optional path.
    pub fn is_remote(self) -> bool {
        !matches!(self, ProviderKind::Ollama)
    }

    /// The keychain key under which this provider's API key is stored
    /// (`None` for the keyless local provider).
    pub fn key_id(self) -> Option<&'static str> {
        match self {
            ProviderKind::Ollama => None,
            ProviderKind::OpenAi => Some("ai-openai-key"),
            ProviderKind::Anthropic => Some("ai-anthropic-key"),
            ProviderKind::Gemini => Some("ai-gemini-key"),
            ProviderKind::AzureOpenAi => Some("ai-azure-key"),
            ProviderKind::Mistral => Some("ai-mistral-key"),
        }
    }

    /// A sensible default model id for the provider.
    pub fn default_model(self) -> &'static str {
        match self {
            ProviderKind::Ollama => "llama3.1",
            ProviderKind::OpenAi => "gpt-4o-mini",
            ProviderKind::Anthropic => "claude-3-5-sonnet-latest",
            ProviderKind::Gemini => "gemini-1.5-flash",
            ProviderKind::AzureOpenAi => "gpt-4o-mini",
            ProviderKind::Mistral => "mistral-small-latest",
        }
    }
}

/// Persisted AI configuration (serde → host settings). Default is AI *off* and
/// no active provider until the user configures one (ADR-0009).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiConfig {
    /// The active provider, or `None` until the user picks one.
    #[serde(default)]
    pub active: Option<ProviderKind>,
    /// Per-task model overrides (keyed by [`AiTask::id`]); falls back to the
    /// active provider's default model when absent.
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, String>,
    /// Default model for the active provider (overrides the built-in default).
    #[serde(default)]
    pub default_model: Option<String>,
    /// Ollama host base URL (local path).
    #[serde(default = "default_ollama_host")]
    pub ollama_host: String,
    /// Azure OpenAI resource endpoint (e.g. `https://my.openai.azure.com`).
    #[serde(default)]
    pub azure_endpoint: String,
    /// Azure OpenAI deployment name (the path-scoped model).
    #[serde(default)]
    pub azure_deployment: String,
    /// Provider kinds for which the user has granted remote-send consent
    /// (ADR-0009). A remote call is blocked until its kind is listed here.
    #[serde(default)]
    pub consented: Vec<ProviderKind>,
}

fn default_ollama_host() -> String {
    "http://localhost:11434".to_string()
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            active: None,
            models: std::collections::BTreeMap::new(),
            default_model: None,
            ollama_host: default_ollama_host(),
            azure_endpoint: String::new(),
            azure_deployment: String::new(),
            consented: Vec::new(),
        }
    }
}

impl AiConfig {
    /// The model id to use for `task`: the per-task override, else the configured
    /// default, else the active provider's built-in default.
    pub fn model_for(&self, task: AiTask) -> String {
        if let Some(m) = self.models.get(task.id()) {
            return m.clone();
        }
        if let Some(m) = &self.default_model {
            if !m.is_empty() {
                return m.clone();
            }
        }
        self.active
            .map(|p| p.default_model().to_string())
            .unwrap_or_default()
    }

    /// Whether remote-send consent has been recorded for `kind`. Local Ollama
    /// is always permitted (it does not leave the machine).
    pub fn has_consent(&self, kind: ProviderKind) -> bool {
        !kind.is_remote() || self.consented.contains(&kind)
    }
}

/// A request to a provider. `system`/`prompt` are already context-built and
/// (for remote providers) redacted by the caller.
#[derive(Clone, Debug)]
pub struct AiRequest {
    /// The task this request serves (model selection / shaping).
    pub task: AiTask,
    /// The model id (provider-specific).
    pub model: String,
    /// System / instruction text.
    pub system: String,
    /// The user prompt (context).
    pub prompt: String,
    /// Sampling temperature.
    pub temperature: f32,
    /// Soft cap on output tokens.
    pub max_tokens: u32,
}

impl AiRequest {
    /// A request with sensible defaults for `task`/`model`.
    pub fn new(task: AiTask, model: impl Into<String>) -> Self {
        AiRequest {
            task,
            model: model.into(),
            system: String::new(),
            prompt: String::new(),
            temperature: 0.2,
            max_tokens: 1024,
        }
    }
}

/// A completed response.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AiResponse {
    /// The full generated text (concatenation of streamed deltas).
    pub text: String,
}

/// A cooperative cancellation flag shared with a running completion. Setting it
/// makes the next [`StreamSink::push`] return [`Error::Cancelled`].
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh, un-cancelled token.
    pub fn new() -> Self {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }
    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A streaming sink the provider pushes token deltas into. Each push checks the
/// cancel token first, so a cancelled completion stops cooperatively.
pub struct StreamSink<'a> {
    on_token: &'a mut (dyn FnMut(&str) + Send),
    cancel: CancelToken,
}

impl<'a> StreamSink<'a> {
    /// Build a sink from a token callback and a cancel token.
    pub fn new(on_token: &'a mut (dyn FnMut(&str) + Send), cancel: CancelToken) -> Self {
        StreamSink { on_token, cancel }
    }

    /// Push a token delta. Returns [`Error::Cancelled`] if cancellation was
    /// requested before this push.
    pub fn push(&mut self, delta: &str) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        (self.on_token)(delta);
        Ok(())
    }
}

/// A provider-agnostic AI backend (ADR-0011). One implementation per provider,
/// each a thin `reqwest`/rustls client. `complete` streams deltas into `sink`
/// and returns the full response.
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// A stable provider id (e.g. `openai`, `ollama`).
    fn id(&self) -> &str;

    /// The provider's approximate context window in tokens (for budgeting).
    fn context_window(&self) -> usize;

    /// Whether this provider sends data off the machine (consent gate applies).
    fn is_remote(&self) -> bool;

    /// Run a completion, streaming deltas into `sink`, returning the full text.
    /// Implementations must call `sink.push` for each delta so cancellation is
    /// honored cooperatively.
    async fn complete(&self, req: &AiRequest, sink: &mut StreamSink<'_>) -> Result<AiResponse>;
}

/// Build the provider for `kind` from `cfg` and an optional API key. Returns
/// [`Error::NoKey`] when a remote provider has no key.
pub fn build_provider(
    kind: ProviderKind,
    cfg: &AiConfig,
    api_key: Option<String>,
) -> Result<Box<dyn AiProvider>> {
    match kind {
        ProviderKind::Ollama => Ok(Box::new(OllamaProvider::new(&cfg.ollama_host))),
        ProviderKind::OpenAi => Ok(Box::new(OpenAiProvider::new(require_key(kind, api_key)?))),
        ProviderKind::Anthropic => Ok(Box::new(AnthropicProvider::new(require_key(
            kind, api_key,
        )?))),
        ProviderKind::Gemini => Ok(Box::new(GeminiProvider::new(require_key(kind, api_key)?))),
        ProviderKind::Mistral => Ok(Box::new(MistralProvider::new(require_key(kind, api_key)?))),
        ProviderKind::AzureOpenAi => Ok(Box::new(AzureOpenAiProvider::new(
            &cfg.azure_endpoint,
            &cfg.azure_deployment,
            require_key(kind, api_key)?,
        ))),
    }
}

fn require_key(kind: ProviderKind, api_key: Option<String>) -> Result<String> {
    api_key
        .filter(|k| !k.is_empty())
        .ok_or_else(|| Error::NoKey(kind.label().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake in-crate provider that streams a canned response token by token,
    /// honoring cancellation. Used across the crate's feature tests.
    pub(crate) struct FakeProvider {
        pub canned: String,
        pub remote: bool,
    }

    #[async_trait::async_trait]
    impl AiProvider for FakeProvider {
        fn id(&self) -> &str {
            "fake"
        }
        fn context_window(&self) -> usize {
            8192
        }
        fn is_remote(&self) -> bool {
            self.remote
        }
        async fn complete(
            &self,
            _req: &AiRequest,
            sink: &mut StreamSink<'_>,
        ) -> Result<AiResponse> {
            let mut text = String::new();
            for tok in self.canned.split_inclusive(' ') {
                sink.push(tok)?;
                text.push_str(tok);
            }
            Ok(AiResponse { text })
        }
    }

    #[tokio::test]
    async fn fake_provider_streams_full_text() {
        let p = FakeProvider {
            canned: "feat: add widget".to_string(),
            remote: false,
        };
        let mut chunks: Vec<String> = Vec::new();
        let mut cb = |d: &str| chunks.push(d.to_string());
        let mut sink = StreamSink::new(&mut cb, CancelToken::new());
        let resp = p
            .complete(&AiRequest::new(AiTask::CommitMessage, "x"), &mut sink)
            .await
            .expect("complete");
        assert_eq!(resp.text, "feat: add widget");
        assert!(chunks.len() >= 2, "should stream multiple deltas");
    }

    #[tokio::test]
    async fn cancellation_stops_streaming() {
        let p = FakeProvider {
            canned: "one two three four".to_string(),
            remote: false,
        };
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut cb = |_d: &str| {};
        let mut sink = StreamSink::new(&mut cb, cancel);
        let err = p
            .complete(&AiRequest::new(AiTask::Explain, "x"), &mut sink)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Cancelled));
    }

    #[test]
    fn config_model_selection_precedence() {
        let mut cfg = AiConfig {
            active: Some(ProviderKind::OpenAi),
            ..Default::default()
        };
        // Falls back to provider default.
        assert_eq!(cfg.model_for(AiTask::CommitMessage), "gpt-4o-mini");
        // Configured default overrides.
        cfg.default_model = Some("gpt-4o".to_string());
        assert_eq!(cfg.model_for(AiTask::CommitMessage), "gpt-4o");
        // Per-task override wins.
        cfg.models
            .insert(AiTask::CommitMessage.id().to_string(), "o1".to_string());
        assert_eq!(cfg.model_for(AiTask::CommitMessage), "o1");
        assert_eq!(cfg.model_for(AiTask::Explain), "gpt-4o");
    }

    #[test]
    fn consent_required_only_for_remote() {
        let cfg = AiConfig::default();
        assert!(cfg.has_consent(ProviderKind::Ollama));
        assert!(!cfg.has_consent(ProviderKind::OpenAi));
        let cfg = AiConfig {
            consented: vec![ProviderKind::OpenAi],
            ..Default::default()
        };
        assert!(cfg.has_consent(ProviderKind::OpenAi));
        assert!(!cfg.has_consent(ProviderKind::Anthropic));
    }
}
