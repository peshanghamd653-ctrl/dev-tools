//! Ollama adapter: local models over the /api/chat NDJSON stream.

use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;

use super::{AiError, AiProvider, AiResult, StreamRequest};
use crate::types::AiDelta;

pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

pub struct OllamaProvider {
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Locally installed models, for the model picker.
    pub async fn list_models(&self, base_url: Option<&str>) -> AiResult<Vec<String>> {
        let base = base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/');
        let value: serde_json::Value = self
            .client
            .get(format!("{base}/api/tags"))
            .send()
            .await?
            .json()
            .await?;
        Ok(value["models"]
            .as_array()
            .map(|models| {
                models
                    .iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AiProvider for OllamaProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }

    async fn stream_chat(
        &self,
        req: StreamRequest<'_>,
        tx: &UnboundedSender<AiDelta>,
    ) -> AiResult<String> {
        let base = req
            .base_url
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/');

        let mut messages: Vec<serde_json::Value> = Vec::new();
        if let Some(system) = req.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.extend(
            req.messages
                .iter()
                .map(|m| serde_json::json!({"role": m.role, "content": m.content})),
        );

        let response = self
            .client
            .post(format!("{base}/api/chat"))
            .json(&serde_json::json!({
                "model": req.model,
                "messages": messages,
                "stream": true,
            }))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AiError::Api {
                provider: "ollama",
                status: status.as_u16(),
                body,
            });
        }

        let mut full = String::new();
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();
        'outer: while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                if let Some((text, done)) = parse_ndjson_line(line.trim()) {
                    if !text.is_empty() {
                        full.push_str(&text);
                        let _ = tx.send(AiDelta::Text { text });
                    }
                    if done {
                        break 'outer;
                    }
                }
            }
        }
        Ok(full)
    }
}

/// Parse one NDJSON line into (text, done).
pub(crate) fn parse_ndjson_line(line: &str) -> Option<(String, bool)> {
    if line.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let text = value["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let done = value["done"].as_bool().unwrap_or(false);
    Some((text, done))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_and_done() {
        let mid =
            r#"{"model":"llama3.2","message":{"role":"assistant","content":"Hi"},"done":false}"#;
        assert_eq!(parse_ndjson_line(mid), Some(("Hi".into(), false)));

        let end = r#"{"model":"llama3.2","message":{"role":"assistant","content":""},"done":true}"#;
        assert_eq!(parse_ndjson_line(end), Some((String::new(), true)));

        assert_eq!(parse_ndjson_line(""), None);
        assert_eq!(parse_ndjson_line("not json"), None);
    }
}
