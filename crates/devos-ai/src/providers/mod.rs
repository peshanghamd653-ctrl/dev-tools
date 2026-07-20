pub mod claude;
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

/// The providers wired into DevOS. Claude and Ollama first (user priority);
/// OpenAI/Gemini slot in behind the same trait later.
pub struct AiRegistry {
    pub claude: claude::ClaudeProvider,
    pub ollama: ollama::OllamaProvider,
}

impl AiRegistry {
    pub fn new() -> Self {
        Self {
            claude: claude::ClaudeProvider::new(),
            ollama: ollama::OllamaProvider::new(),
        }
    }

    pub fn provider(&self, id: &str) -> AiResult<&dyn AiProvider> {
        match id {
            "claude" => Ok(&self.claude),
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
