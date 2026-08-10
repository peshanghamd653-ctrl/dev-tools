pub mod claude;
pub mod gemini;
pub mod ollama;

use tokio::sync::mpsc::UnboundedSender;

use crate::types::{AiDelta, ChatTurn};

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("no API key configured for {0}")]
    MissingKey(&'static str),
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{provider} API error ({status}): {body}")]
    Api {
        provider: &'static str,
        status: u16,
        body: String,
    },
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type AiResult<T> = Result<T, AiError>;

pub struct StreamRequest<'a> {
    pub model: &'a str,
    pub system: Option<&'a str>,
    pub messages: &'a [ChatTurn],
    /// API key for cloud providers.
    pub api_key: Option<&'a str>,
    /// Base URL override (Ollama host).
    pub base_url: Option<&'a str>,
}

/// A tool the model may call, in provider-neutral JSON-schema form.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Executes tool calls on behalf of the agent loop. Implemented by the
/// desktop layer, which owns filesystem scoping and (later) approval UX.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, input: &serde_json::Value) -> Result<String, String>;
}

#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Stream a reply, emitting [`AiDelta::Text`] frames as tokens arrive.
    /// Returns the full assistant message on completion.
    async fn stream_chat(
        &self,
        req: StreamRequest<'_>,
        tx: &UnboundedSender<AiDelta>,
    ) -> AiResult<String>;
}

/// The providers wired into DevOS: Claude, Gemini and Ollama. OpenAI slots in
/// behind the same trait when there is a reason to add it.
///
/// All three drive the agentic tool loop — see `claude::run_agent`,
/// `ollama::run_agent` and `gemini::run_agent`, three independent
/// implementations rather than one generic loop, because the APIs disagree
/// on how a tool call is represented (SSE content blocks with
/// incrementally-streamed argument JSON vs. a single NDJSON frame carrying
/// the call complete vs. a whole `functionCall` part) and on how the result
/// is threaded back into the conversation (`tool_result` content blocks vs.
/// a `role: "tool"` message vs. a `functionResponse` part in a `user`-role
/// `Content`, since Gemini's schema has no third "tool" role at all).
pub struct AiRegistry {
    pub claude: claude::ClaudeProvider,
    pub gemini: gemini::GeminiProvider,
    pub ollama: ollama::OllamaProvider,
}

impl AiRegistry {
    pub fn new() -> Self {
        Self {
            claude: claude::ClaudeProvider::new(),
            gemini: gemini::GeminiProvider::new(),
            ollama: ollama::OllamaProvider::new(),
        }
    }

    pub fn provider(&self, id: &str) -> AiResult<&dyn AiProvider> {
        match id {
            "claude" => Ok(&self.claude),
            "gemini" => Ok(&self.gemini),
            "ollama" => Ok(&self.ollama),
            other => Err(AiError::UnknownProvider(other.to_string())),
        }
    }

    /// One-shot completion: no streaming consumer, just the final text.
    pub async fn complete_once(
        &self,
        provider_id: &str,
        req: StreamRequest<'_>,
    ) -> AiResult<String> {
        let provider = self.provider(provider_id)?;
        // Providers tolerate a closed channel (`let _ = tx.send(..)`), so
        // dropping the receiver immediately turns streaming into a no-op.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        provider.stream_chat(req, &tx).await
    }
}

impl Default for AiRegistry {
    fn default() -> Self {
        Self::new()
    }
}
