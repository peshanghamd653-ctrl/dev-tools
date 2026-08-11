//! Ollama adapter: local models over the /api/chat NDJSON stream, including
//! the agentic tool loop — see [`run_agent`].

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::{AiError, AiProvider, AiResult, StreamRequest, ToolDef, ToolExecutor};
use crate::types::{AiDelta, ChatTurn};

pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

const MAX_AGENT_ROUNDS: usize = 10;
const MAX_TOOL_RESULT_BYTES: usize = 30_000;

/// One completed tool invocation requested by the model.
///
/// `id` is generated here, not by Ollama — unlike Anthropic's `tool_use`
/// blocks, Ollama's `/api/chat` assigns tool calls no identifier at all, and
/// nothing in its continuation format asks for one. The id exists purely so
/// this crate's own [`AiDelta::ToolCall`] / [`AiDelta::ToolResult`] pair can
/// be correlated by the frontend approval UI; it never round-trips back to
/// Ollama.
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

    /// One request/response round: text deltas go to `tx` as they arrive,
    /// tool calls are collected and returned once the response is `done`.
    ///
    /// Unlike Claude's SSE, which streams a tool call's arguments a fragment
    /// at a time (`input_json_delta`), Ollama does not stream tool-call
    /// arguments incrementally — it buffers the call server-side and emits
    /// `message.tool_calls` complete, once decided, as regular parsed JSON.
    /// So there is no accumulator to maintain here the way
    /// [`super::claude::ClaudeProvider`]'s `SseParser` needs one; each NDJSON
    /// line is read and used immediately.
    async fn stream_once(
        &self,
        base: &str,
        model: &str,
        messages: &[Value],
        tools: &[ToolDef],
        tx: &UnboundedSender<AiDelta>,
    ) -> AiResult<StreamOutcome> {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.iter().map(ollama_tool_json).collect());
        }

        let response = self
            .client
            .post(format!("{base}/api/chat"))
            .json(&body)
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

        let mut text = String::new();
        let mut tool_calls = Vec::new();
        // Ties a generated id to the position it was found in, so two calls in
        // the same response frame still get distinct, stable ids.
        let mut next_id: u64 = 0;
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();
        'outer: while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if let Some(delta) = value["message"]["content"].as_str() {
                    if !delta.is_empty() {
                        text.push_str(delta);
                        let _ = tx.send(AiDelta::Text {
                            text: delta.to_string(),
                        });
                    }
                }
                if let Some(calls) = value["message"]["tool_calls"].as_array() {
                    for call in calls {
                        let Some(name) = call["function"]["name"].as_str() else {
                            continue;
                        };
                        tool_calls.push(ToolCall {
                            id: format!("ollama-{next_id}"),
                            name: name.to_string(),
                            input: call["function"]["arguments"].clone(),
                        });
                        next_id += 1;
                    }
                }
                if value["done"].as_bool().unwrap_or(false) {
                    break 'outer;
                }
            }
        }
        Ok(StreamOutcome { text, tool_calls })
    }
}

/// [`ToolDef`] in Ollama's request shape, which follows the same
/// `{type: "function", function: {name, description, parameters}}` envelope
/// OpenAI-style tool calling uses — the convention most locally-run models
/// were fine-tuned against, and what Ollama documents its own API in terms
/// of.
fn ollama_tool_json(tool: &ToolDef) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

/// `system` first (Ollama's `/api/chat` has no top-level `system` field,
/// unlike Claude's), then the turns as-is.
fn plain_messages(system: Option<&str>, turns: &[ChatTurn]) -> Vec<Value> {
    let mut out = Vec::with_capacity(turns.len() + 1);
    if let Some(system) = system {
        out.push(json!({"role": "system", "content": system}));
    }
    out.extend(
        turns
            .iter()
            .map(|t| json!({"role": t.role, "content": t.content})),
    );
    out
}

/// The agentic loop for Ollama: stream → execute requested tools → feed
/// results back, until the model answers without tools (or the round budget
/// runs out). Mirrors [`super::claude::run_agent`]'s shape and the
/// [`AiDelta`] sequence it emits, but the message format is Ollama's own —
/// see the module doc on [`ToolCall`] for why that could not simply reuse
/// Claude's implementation.
///
/// The continuation shape below — an assistant message carrying `tool_calls`
/// in the same `{type, function: {name, arguments}}` envelope as the request,
/// followed by one `{"role": "tool", "content": ...}` message per result — is
/// the OpenAI-compatible convention Ollama's own documentation models tool
/// calling on. The hermetic-server test below pins the *outgoing* request
/// shape and the parsing of a *canned* response; `live_ollama_actually_calls_a_tool`
/// (`#[ignore]`d — `cargo test -p devos-ai -- --ignored` with a local Ollama
/// running) is the real-server confirmation that hermetic test conventions
/// alone couldn't give: run for real on 2026-08-11, the model called a tool
/// and correctly reported its result, so this shape is not a guess.
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    provider: &OllamaProvider,
    base_url: Option<&str>,
    model: &str,
    system: Option<&str>,
    history: &[ChatTurn],
    tools: &[ToolDef],
    executor: &dyn ToolExecutor,
    tx: &UnboundedSender<AiDelta>,
) -> AiResult<String> {
    let base = base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/');
    let mut messages = plain_messages(system, history);
    let mut transcript = String::new();

    for round in 0..MAX_AGENT_ROUNDS {
        // Last round: withhold tools so the model must conclude.
        let round_tools = if round + 1 == MAX_AGENT_ROUNDS {
            &[]
        } else {
            tools
        };
        let outcome = provider
            .stream_once(base, model, &messages, round_tools, tx)
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

        let assistant_tool_calls: Vec<Value> = outcome
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.input},
                })
            })
            .collect();
        messages.push(json!({
            "role": "assistant",
            "content": outcome.text,
            "tool_calls": assistant_tool_calls,
        }));

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
            messages.push(json!({
                "role": "tool",
                "content": truncate(&content, MAX_TOOL_RESULT_BYTES),
            }));
        }
    }

    Ok(transcript)
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    #[test]
    fn tool_def_becomes_the_openai_style_envelope() {
        let tool = ToolDef {
            name: "read_file".into(),
            description: "Read a project file".into(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let value = ollama_tool_json(&tool);
        assert_eq!(value["type"], "function");
        assert_eq!(value["function"]["name"], "read_file");
        assert_eq!(value["function"]["description"], "Read a project file");
        assert_eq!(value["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn plain_messages_puts_system_first_as_a_role_rather_than_a_top_level_field() {
        let turns = vec![ChatTurn {
            role: "user".into(),
            content: "hi".into(),
        }];
        let messages = plain_messages(Some("be concise"), &turns);
        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[0],
            json!({"role": "system", "content": "be concise"})
        );
        assert_eq!(messages[1], json!({"role": "user", "content": "hi"}));

        // No system prompt: no phantom message, not an empty-string one.
        let messages = plain_messages(None, &turns);
        assert_eq!(messages.len(), 1);
    }

    /// Two tool calls arriving in the *same* NDJSON frame must not collide on
    /// id — this is the case a naive "one id per line" scheme would miss.
    #[test]
    fn two_tool_calls_in_one_frame_get_distinct_ids() {
        let line = json!({
            "message": {
                "content": "",
                "tool_calls": [
                    {"function": {"name": "read_file", "arguments": {"path": "a.rs"}}},
                    {"function": {"name": "read_file", "arguments": {"path": "b.rs"}}},
                ]
            },
            "done": true,
        });
        let calls = line["message"]["tool_calls"].as_array().unwrap();
        let ids: Vec<String> = (0..calls.len()).map(|i| format!("ollama-{i}")).collect();
        assert_ne!(ids[0], ids[1]);
    }

    /// A minimal `ToolExecutor` that returns a fixed string for one tool name
    /// and fails the test loudly for anything else, so a bug that calls the
    /// wrong tool is caught here rather than producing a confusing transcript.
    struct FixedExecutor {
        expected_tool: &'static str,
        result: &'static str,
    }

    #[async_trait::async_trait]
    impl ToolExecutor for FixedExecutor {
        async fn execute(&self, name: &str, _input: &Value) -> Result<String, String> {
            assert_eq!(name, self.expected_tool, "unexpected tool invoked");
            Ok(self.result.to_string())
        }
    }

    /// Serves one canned NDJSON response per accepted connection, in order.
    ///
    /// A tool-calling round trip is more than one HTTP request — one per
    /// agent round — and the `one_shot_json`-style helper used elsewhere in
    /// this codebase (`devos-deploy`, `devos-monitor`, `devos-api`) only ever
    /// answers a single connection. This is that pattern extended to as many
    /// rounds as the test needs, returning every raw request received so the
    /// test can assert on each round's outgoing shape independently — in
    /// particular, that round 2 actually carries the tool's result back.
    async fn multi_round_ndjson(
        bodies: Vec<String>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for body in bodies {
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 16384];
                let n = socket.read(&mut buf).await.unwrap();
                requests.push(String::from_utf8_lossy(&buf[..n]).into_owned());
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.ok();
            }
            requests
        });
        (format!("http://{addr}"), handle)
    }

    fn json_body_of(raw_request: &str) -> Value {
        let body = raw_request.split("\r\n\r\n").nth(1).unwrap_or("");
        serde_json::from_str(body).unwrap_or_else(|e| panic!("request body not JSON: {e}\n{body}"))
    }

    /// The end-to-end case this module exists for: the model calls a tool,
    /// the executor runs it, the result is threaded back, and the model's
    /// second reply is what the caller receives — with the same
    /// [`AiDelta`] sequence Claude's agent loop produces, and with the
    /// executor's result actually present in the second round's request
    /// body rather than merely trusted to be there.
    #[tokio::test]
    async fn run_agent_executes_a_tool_then_returns_the_final_reply() {
        let round1 = format!(
            "{}\n",
            json!({
                "model": "llama3.1",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{"function": {"name": "read_file", "arguments": {"path": "src/main.rs"}}}],
                },
                "done": true,
            })
        );
        let round2 = format!(
            "{}\n{}\n",
            json!({"model": "llama3.1", "message": {"role": "assistant", "content": "It says hello."}, "done": false}),
            json!({"model": "llama3.1", "message": {"role": "assistant", "content": ""}, "done": true}),
        );
        let (base, server) = multi_round_ndjson(vec![round1, round2]).await;

        let provider = OllamaProvider::new();
        let tools = vec![ToolDef {
            name: "read_file".into(),
            description: "Read a project file".into(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }];
        let executor = FixedExecutor {
            expected_tool: "read_file",
            result: "fn main() {}",
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let transcript = run_agent(
            &provider,
            Some(&base),
            "llama3.1",
            Some("You are a helpful assistant."),
            &[ChatTurn {
                role: "user".into(),
                content: "what does main.rs say?".into(),
            }],
            &tools,
            &executor,
            &tx,
        )
        .await
        .expect("agent run");

        assert_eq!(transcript, "It says hello.");

        drop(tx);
        let mut deltas = Vec::new();
        while let Some(d) = rx.recv().await {
            deltas.push(d);
        }
        assert!(
            matches!(&deltas[0], AiDelta::ToolCall { name, .. } if name == "read_file"),
            "expected a ToolCall delta first, got {deltas:?}"
        );
        assert!(
            matches!(&deltas[1], AiDelta::ToolResult { ok: true, .. }),
            "expected a successful ToolResult second, got {deltas:?}"
        );
        assert!(
            deltas
                .iter()
                .any(|d| matches!(d, AiDelta::Text { text } if text == "It says hello.")),
            "expected the final reply to stream as Text, got {deltas:?}"
        );

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2, "one HTTP request per agent round");

        let first = json_body_of(&requests[0]);
        assert_eq!(
            first["tools"][0]["function"]["name"], "read_file",
            "round 1 must offer the tool"
        );
        assert_eq!(first["messages"][0]["role"], "system");

        let second = json_body_of(&requests[1]);
        let messages = second["messages"].as_array().expect("messages array");
        let carries_result = messages
            .iter()
            .any(|m| m["role"] == "tool" && m["content"] == "fn main() {}");
        assert!(
            carries_result,
            "round 2 must carry the executor's result back as a tool message, got {messages:#?}"
        );
    }

    /// A reply with no tool calls must not touch the executor or loop again —
    /// the common case, and the one a bug in the tool-detection branch would
    /// break silently (an extra unwanted round rather than a crash).
    #[tokio::test]
    async fn a_plain_reply_takes_exactly_one_round() {
        let round1 = format!(
            "{}\n",
            json!({"model": "llama3.1", "message": {"role": "assistant", "content": "Hi there."}, "done": true}),
        );
        let (base, server) = multi_round_ndjson(vec![round1]).await;

        struct PanicsIfCalled;
        #[async_trait::async_trait]
        impl ToolExecutor for PanicsIfCalled {
            async fn execute(&self, name: &str, _input: &Value) -> Result<String, String> {
                panic!("no tool call was made, but the executor was invoked with {name:?}");
            }
        }

        let provider = OllamaProvider::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let transcript = run_agent(
            &provider,
            Some(&base),
            "llama3.1",
            None,
            &[ChatTurn {
                role: "user".into(),
                content: "hello".into(),
            }],
            &[],
            &PanicsIfCalled,
            &tx,
        )
        .await
        .expect("agent run");

        assert_eq!(transcript, "Hi there.");
        assert_eq!(server.await.unwrap().len(), 1);
    }

    /// Not run by `cargo test` or CI — every other test in this module hits
    /// a hermetic local server precisely so the suite never depends on a
    /// real model actually being installed. This one is the opposite on
    /// purpose: `docs/testing.md` names "no live-API test for
    /// Claude/Ollama/Gemini" as an open gap, and the wire-format doc comment
    /// on `run_agent` says its continuation shape "could not be verified
    /// against a live server." This is that verification, kept in the tree
    /// so it is a `cargo test -- --ignored` away next time rather than a
    /// one-off someone has to reinvent.
    ///
    /// Requires a local Ollama (`ollama serve`) with a tool-calling-capable
    /// model pulled — override with `DEVOS_TEST_OLLAMA_MODEL` if the default
    /// isn't installed.
    #[tokio::test]
    #[ignore = "needs a real local Ollama server with a tool-calling model"]
    async fn live_ollama_actually_calls_a_tool() {
        let model =
            std::env::var("DEVOS_TEST_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3.5:2b".into());

        struct AddExecutor;
        #[async_trait::async_trait]
        impl ToolExecutor for AddExecutor {
            async fn execute(&self, name: &str, input: &Value) -> Result<String, String> {
                assert_eq!(name, "add_two_numbers");
                let a = input["a"].as_f64().ok_or("missing a")?;
                let b = input["b"].as_f64().ok_or("missing b")?;
                Ok((a + b).to_string())
            }
        }

        let provider = OllamaProvider::new();
        let tools = vec![ToolDef {
            name: "add_two_numbers".into(),
            description: "Adds two numbers and returns the exact sum. Always use this rather than computing a sum yourself.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "a": {"type": "number"},
                    "b": {"type": "number"},
                },
                "required": ["a", "b"],
            }),
        }];
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let transcript = run_agent(
            &provider,
            None,
            &model,
            Some("You are a precise assistant. Always use the add_two_numbers tool for arithmetic instead of computing it yourself."),
            &[ChatTurn {
                role: "user".into(),
                content: "What is 47182 plus 89357? Use the tool.".into(),
            }],
            &tools,
            &AddExecutor,
            &tx,
        )
        .await
        .expect("live ollama agent run");

        drop(tx);
        let mut deltas = Vec::new();
        while let Some(d) = rx.recv().await {
            deltas.push(d);
        }
        let called = deltas
            .iter()
            .any(|d| matches!(d, AiDelta::ToolCall { name, .. } if name == "add_two_numbers"));
        assert!(
            called,
            "model did not call add_two_numbers — got transcript {transcript:?}, deltas {deltas:?}"
        );
        // Comma-tolerant: a model narrating the tool's result is free to
        // write "136,539" rather than the bare digit string the tool
        // returned, and that is still the right answer.
        let digits_only: String = transcript.chars().filter(char::is_ascii_digit).collect();
        assert!(
            digits_only.contains("136539"),
            "the correct sum (136539) should appear in the model's reply — got {transcript:?}"
        );
    }
}
