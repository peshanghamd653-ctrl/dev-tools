//! Claude adapter: Anthropic Messages API with SSE streaming and tool use.

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::{AiError, AiProvider, AiResult, StreamRequest, ToolDef, ToolExecutor};
use crate::types::{AiDelta, ChatTurn};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MAX_AGENT_ROUNDS: usize = 10;
const MAX_TOOL_RESULT_BYTES: usize = 30_000;

pub const CLAUDE_MODELS: &[&str] = &["claude-sonnet-5", "claude-opus-4-8", "claude-haiku-4-5"];
pub const DEFAULT_MODEL: &str = "claude-sonnet-5";

pub struct ClaudeProvider {
    client: reqwest::Client,
}

/// One completed tool invocation requested by the model.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Result of a single streamed request.
struct StreamOutcome {
    text: String,
    tool_calls: Vec<ToolCall>,
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// One streamed request. Text deltas go to `tx` as they arrive; tool_use
    /// blocks are accumulated and returned.
    async fn stream_once(
        &self,
        api_key: &str,
        model: &str,
        system: Option<&str>,
        messages: &[Value],
        tools: &[ToolDef],
        tx: &UnboundedSender<AiDelta>,
    ) -> AiResult<StreamOutcome> {
        let mut body = json!({
            "model": model,
            "max_tokens": 4096,
            "stream": true,
            "messages": messages,
        });
        if let Some(system) = system {
            body["system"] = Value::String(system.to_string());
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(
                tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "input_schema": t.input_schema,
                        })
                    })
                    .collect(),
            );
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

        let mut parser = SseParser::default();
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                if let Some(text) = parser.feed(line.trim_end()) {
                    let _ = tx.send(AiDelta::Text { text });
                }
            }
        }
        Ok(parser.finish())
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
        let messages = plain_messages(req.messages);
        let outcome = self
            .stream_once(api_key, req.model, req.system, &messages, &[], tx)
            .await?;
        Ok(outcome.text)
    }
}

/// The agentic loop: stream → execute requested tools → feed results back,
/// until the model answers without tools (or the round budget runs out).
/// Read-only tools only for now; the caller decides which tools exist at all
/// (that decision is the user's grant).
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    provider: &ClaudeProvider,
    api_key: &str,
    model: &str,
    system: Option<&str>,
    history: &[ChatTurn],
    tools: &[ToolDef],
    executor: &dyn ToolExecutor,
    tx: &UnboundedSender<AiDelta>,
) -> AiResult<String> {
    let mut messages = plain_messages(history);
    let mut transcript = String::new();

    for round in 0..MAX_AGENT_ROUNDS {
        // Last round: withhold tools so the model must conclude.
        let round_tools = if round + 1 == MAX_AGENT_ROUNDS {
            &[]
        } else {
            tools
        };
        let outcome = provider
            .stream_once(api_key, model, system, &messages, round_tools, tx)
            .await?;

        if !outcome.text.is_empty() {
            if !transcript.is_empty() {
                transcript.push_str("\n\n");
            }
            transcript.push_str(&outcome.text);
        }
        if outcome.tool_calls.is_empty() {
            break;
        }

        // Echo the assistant turn (text + tool_use blocks) back verbatim.
        let mut assistant_content = Vec::new();
        if !outcome.text.is_empty() {
            assistant_content.push(json!({"type": "text", "text": outcome.text}));
        }
        for call in &outcome.tool_calls {
            assistant_content.push(json!({
                "type": "tool_use", "id": call.id, "name": call.name, "input": call.input,
            }));
        }
        messages.push(json!({"role": "assistant", "content": assistant_content}));

        let mut results = Vec::new();
        for call in &outcome.tool_calls {
            let _ = tx.send(AiDelta::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.input.to_string(),
            });
            let executed = executor.execute(&call.name, &call.input).await;
            let (ok, content) = match executed {
                Ok(content) => (true, content),
                Err(error) => (false, error),
            };
            let _ = tx.send(AiDelta::ToolResult {
                id: call.id.clone(),
                ok,
                summary: truncate(&content, 160),
            });
            results.push(json!({
                "type": "tool_result",
                "tool_use_id": call.id,
                "content": truncate(&content, MAX_TOOL_RESULT_BYTES),
                "is_error": !ok,
            }));
        }
        messages.push(json!({"role": "user", "content": results}));
    }

    Ok(transcript)
}

fn plain_messages(turns: &[ChatTurn]) -> Vec<Value> {
    turns
        .iter()
        .map(|t| json!({"role": t.role, "content": t.content}))
        .collect()
}

/// Stateful SSE parser: emits text deltas immediately, accumulates tool_use
/// blocks (id/name from content_block_start, arguments from input_json_delta).
#[derive(Default)]
struct SseParser {
    text: String,
    /// index → (id, name, json buffer)
    pending_tools: Vec<(u64, String, String, String)>,
}

impl SseParser {
    /// Feed one SSE line. Returns a text delta if the line carried one.
    fn feed(&mut self, line: &str) -> Option<String> {
        let data = line.strip_prefix("data: ")?;
        let value: Value = serde_json::from_str(data).ok()?;
        match value["type"].as_str()? {
            "content_block_start" => {
                let block = &value["content_block"];
                if block["type"] == "tool_use" {
                    self.pending_tools.push((
                        value["index"].as_u64()?,
                        block["id"].as_str()?.to_string(),
                        block["name"].as_str()?.to_string(),
                        String::new(),
                    ));
                }
                None
            }
            "content_block_delta" => match value["delta"]["type"].as_str()? {
                "text_delta" => {
                    let text = value["delta"]["text"].as_str()?.to_string();
                    self.text.push_str(&text);
                    Some(text)
                }
                "input_json_delta" => {
                    let index = value["index"].as_u64()?;
                    let partial = value["delta"]["partial_json"].as_str()?;
                    if let Some(entry) = self.pending_tools.iter_mut().find(|(i, ..)| *i == index) {
                        entry.3.push_str(partial);
                    }
                    None
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn finish(self) -> StreamOutcome {
        let tool_calls = self
            .pending_tools
            .into_iter()
            .map(|(_, id, name, buffer)| ToolCall {
                id,
                name,
                input: if buffer.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&buffer).unwrap_or(json!({}))
                },
            })
            .collect();
        StreamOutcome {
            text: self.text,
            tool_calls,
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_text_deltas() {
        let mut parser = SseParser::default();
        let delta = parser.feed(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        );
        assert_eq!(delta.as_deref(), Some("Hello"));
        assert_eq!(parser.feed("event: message_start"), None);
        let outcome = parser.finish();
        assert_eq!(outcome.text, "Hello");
        assert!(outcome.tool_calls.is_empty());
    }

    #[test]
    fn accumulates_tool_use_across_split_json_deltas() {
        let mut parser = SseParser::default();
        parser.feed(
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"read_file"}}"#,
        );
        parser.feed(
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\": \"src/"}}"#,
        );
        parser.feed(
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"main.rs\"}"}}"#,
        );
        let outcome = parser.finish();
        assert_eq!(outcome.tool_calls.len(), 1);
        let call = &outcome.tool_calls[0];
        assert_eq!(call.id, "tu_1");
        assert_eq!(call.name, "read_file");
        assert_eq!(call.input["path"], "src/main.rs");
    }

    #[test]
    fn text_and_tools_interleave() {
        let mut parser = SseParser::default();
        parser.feed(
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me check."}}"#,
        );
        parser.feed(
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_2","name":"list_dir"}}"#,
        );
        parser.feed(
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
        );
        let outcome = parser.finish();
        assert_eq!(outcome.text, "Let me check.");
        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.tool_calls[0].input, json!({}));
    }

    #[test]
    fn empty_tool_input_defaults_to_object() {
        let mut parser = SseParser::default();
        parser.feed(
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tu_3","name":"list_dir"}}"#,
        );
        let outcome = parser.finish();
        assert_eq!(outcome.tool_calls[0].input, json!({}));
    }
}
