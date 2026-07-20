//! Claude adapter: Anthropic Messages API with SSE streaming.

use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use super::{AiError, AiProvider, AiResult, StreamRequest};
use crate::types::AiDelta;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub const CLAUDE_MODELS: &[&str] = &["claude-sonnet-5", "claude-opus-4-8", "claude-haiku-4-5"];
pub const DEFAULT_MODEL: &str = "claude-sonnet-5";

pub struct ClaudeProvider {
    client: reqwest::Client,
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AiProvider for ClaudeProvider {
    fn id(&self) -> &'static str {
        "claude"
    }

    async fn stream_chat(
        &self,
        req: StreamRequest<'_>,
        tx: &UnboundedSender<AiDelta>,
    ) -> AiResult<String> {
        let api_key = req.api_key.ok_or(AiError::MissingKey("claude"))?;

        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": 4096,
            "stream": true,
            "messages": req.messages,
        });
        if let Some(system) = req.system {
            body["system"] = serde_json::Value::String(system.to_string());
        }

        let response = self
            .client
            .post(API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", API_VERSION)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AiError::Api {
                provider: "claude",
                status: status.as_u16(),
                body: truncate(&body, 500),
            });
        }

        let mut full = String::new();
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            // Process complete lines; keep the trailing partial line buffered.
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                if let Some(text) = parse_sse_line(line.trim_end()) {
                    full.push_str(&text);
                    let _ = tx.send(AiDelta::Text { text });
                }
            }
        }
        Ok(full)
    }
}

/// Extract the text delta from one SSE line, if it carries one.
pub(crate) fn parse_sse_line(line: &str) -> Option<String> {
    let data = line.strip_prefix("data: ")?;
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    if value["type"] == "content_block_delta" && value["delta"]["type"] == "text_delta" {
        value["delta"]["text"].as_str().map(String::from)
    } else {
        None
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", &s[..max])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_deltas() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        assert_eq!(parse_sse_line(line).as_deref(), Some("Hello"));
    }

    #[test]
    fn ignores_non_delta_events() {
        assert_eq!(parse_sse_line("event: message_start"), None);
        assert_eq!(parse_sse_line(r#"data: {"type":"message_stop"}"#), None);
        assert_eq!(
            parse_sse_line(
                r#"data: {"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{"}}"#
            ),
            None
        );
        assert_eq!(parse_sse_line(""), None);
    }
}
