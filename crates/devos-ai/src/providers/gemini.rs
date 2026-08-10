//! Gemini adapter: Google's Generative Language API with SSE streaming,
//! including the agentic tool loop — see [`run_agent`].
//!
//! Wired because the free tier makes it the one cloud provider a user can try
//! without a billing account — Claude needs one, and Ollama needs hardware.
//!
//! Three things differ from the Claude adapter and are worth knowing before
//! editing this file:
//!
//! * **The assistant role is called `model`.** Sending `"assistant"` is not a
//!   validation error; it is accepted and produces a worse reply, which is the
//!   kind of bug that never gets found. Mapping happens in one place.
//! * **The system prompt is not a message.** It goes in `systemInstruction`,
//!   outside `contents`.
//! * **The API key goes in a header, not the query string.** `?key=` is
//!   documented and works, but URLs end up in logs, proxies and error
//!   messages. `x-goog-api-key` keeps it out of all three.
//!
//! # Tool calling
//!
//! Gemini's function calling is a third shape, different from both Claude's
//! (`tool_use` SSE content blocks with incrementally-streamed argument JSON)
//! and Ollama's (a single NDJSON frame carrying the call complete):
//!
//! * A tool call arrives as a `functionCall` [`Part`][part] — `{name, args}`
//!   — inside a `model`-role [`Content`][content], same as `stream_once`
//!   observes it: complete in one part, not streamed incrementally the way
//!   Claude's argument JSON is.
//! * The result goes back as a `functionResponse` part — `{name, response}`
//!   — inside a `user`-role `Content` (there is no third "tool" role; the
//!   schema only has `user` and `model`). `response` is an arbitrary object;
//!   `{"content": <result>}` is the field this uses.
//!
//! [part]: https://ai.google.dev/api/caching#Part
//! [content]: https://ai.google.dev/api/caching#Content
//!
//! This shape could not be verified against a live server in the environment
//! this was written in — confirmed against Google's REST reference and a
//! third-party client's type definitions, not a real API key. The hermetic
//! tests below pin the outgoing request and the parsing of a canned
//! response; if a real model's continuation expectations differ, this
//! module is the one place to reconcile them.

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::{AiError, AiProvider, AiResult, StreamRequest, ToolDef, ToolExecutor};
use crate::types::{AiDelta, ChatTurn};

/// Overridable so tests can point at a local server, and so a user behind a
/// proxy has somewhere to aim. `v1beta` is where `generateContent` lives.
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

const MAX_AGENT_ROUNDS: usize = 10;
const MAX_TOOL_RESULT_BYTES: usize = 30_000;

/// Free-tier-friendly models, newest first. All are flash-class: the pro
/// models are the ones a free key cannot usefully drive, so offering them
/// here would mostly produce quota errors.
pub const GEMINI_MODELS: &[&str] = &[
    "gemini-3.6-flash",
    "gemini-3.5-flash",
    "gemini-3.5-flash-lite",
    "gemini-2.5-flash",
];

pub const DEFAULT_MODEL: &str = "gemini-3.6-flash";

pub struct GeminiProvider {
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// DevOS turns → Gemini `contents`.
///
/// `assistant` becomes `model`; anything else is passed through as `user`,
/// because Gemini rejects unknown roles outright and a rejected request is a
/// better failure than a silently reinterpreted conversation.
fn contents(turns: &[crate::types::ChatTurn]) -> Vec<Value> {
    turns
        .iter()
        .map(|turn| {
            let role = if turn.role == "assistant" {
                "model"
            } else {
                "user"
            };
            json!({ "role": role, "parts": [{ "text": turn.content }] })
        })
        .collect()
}

/// Pull the text out of one streamed `GenerateContentResponse`.
///
/// Every field on the path is optional in practice: a chunk can carry only
/// `safetyRatings`, or a `finishReason` with no parts at all. Returning an
/// empty string for those is correct — they are not errors, they are frames
/// with nothing to display.
fn text_from_chunk(chunk: &Value) -> String {
    chunk
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// One completed tool invocation requested by the model.
///
/// `id` is generated here, not by Gemini — a `functionCall` part carries no
/// identifier of its own, the same gap [`super::ollama::ToolCall`] fills for
/// the same reason. It exists purely to correlate this crate's own
/// [`AiDelta::ToolCall`] / [`AiDelta::ToolResult`] pair for the frontend
/// approval UI; it never round-trips back to Gemini.
#[derive(Debug, Clone)]
struct ToolCall {
    id: String,
    name: String,
    input: Value,
}

/// Result of a single streamed request.
struct StreamOutcome {
    text: String,
    tool_calls: Vec<ToolCall>,
}

/// Pull both text and `functionCall` parts out of one streamed
/// `GenerateContentResponse` chunk, in the order they appear. Separate from
/// [`text_from_chunk`] rather than folding tool support into it: the plain
/// `stream_chat` path never sends `tools` in the request, so a `functionCall`
/// part cannot appear there, and keeping the simpler function means that path
/// stays untouched by this change.
fn parts_from_chunk(chunk: &Value) -> (String, Vec<(String, Value)>) {
    let mut text = String::new();
    let mut calls = Vec::new();
    let parts = chunk
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array);
    if let Some(parts) = parts {
        for part in parts {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                text.push_str(t);
            } else if let Some(call) = part.get("functionCall") {
                let name = call
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                calls.push((name, args));
            }
        }
    }
    (text, calls)
}

/// [`ToolDef`]s in Gemini's request shape: one `tools` array entry holding
/// every declaration, not one entry per tool the way Ollama's OpenAI-style
/// envelope works. `parameters` is passed through as-is from `input_schema`
/// — every tool this app defines uses a plain `{type, properties, required}`
/// shape, which is valid under Gemini's OpenAPI-subset parameter schema, so
/// there is nothing here to translate.
fn gemini_tools_json(tools: &[ToolDef]) -> Value {
    json!([{
        "functionDeclarations": tools
            .iter()
            .map(|t| json!({
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema,
            }))
            .collect::<Vec<_>>(),
    }])
}

/// Byte-safe truncation for the tool-result cap — distinct from [`truncate`]
/// below, which counts *characters* and exists only for the API-error-body
/// case. `MAX_TOOL_RESULT_BYTES` is a byte budget, matching the cap Claude's
/// and Ollama's agent loops both enforce the same way.
fn truncate_bytes(s: &str, max: usize) -> String {
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

impl GeminiProvider {
    /// One request/response round: text deltas go to `tx` as they arrive,
    /// tool calls are collected and returned once the stream ends.
    #[allow(clippy::too_many_arguments)]
    async fn stream_once(
        &self,
        api_key: &str,
        base: &str,
        model: &str,
        system: Option<&str>,
        contents: &[Value],
        tools: &[ToolDef],
        tx: &UnboundedSender<AiDelta>,
    ) -> AiResult<StreamOutcome> {
        let url = format!("{base}/v1beta/models/{model}:streamGenerateContent?alt=sse");

        let mut body = json!({ "contents": contents });
        if let Some(system) = system {
            body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
        }
        if !tools.is_empty() {
            body["tools"] = gemini_tools_json(tools);
        }

        let response = self
            .client
            .post(&url)
            .header("x-goog-api-key", api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AiError::Api {
                provider: "gemini",
                status: status.as_u16(),
                body: truncate(&body, 500),
            });
        }

        let mut text = String::new();
        let mut tool_calls = Vec::new();
        // Ties a generated id to the position it was found in, so two calls
        // arriving across different chunks — or the same one — still get
        // distinct, stable ids.
        let mut next_id: u64 = 0;
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let Some(payload) = line.trim_end().strip_prefix("data: ") else {
                    continue;
                };
                if payload == "[DONE]" {
                    continue;
                }
                let Ok(parsed) = serde_json::from_str::<Value>(payload) else {
                    continue;
                };
                let (chunk_text, chunk_calls) = parts_from_chunk(&parsed);
                if !chunk_text.is_empty() {
                    text.push_str(&chunk_text);
                    let _ = tx.send(AiDelta::Text { text: chunk_text });
                }
                for (name, input) in chunk_calls {
                    tool_calls.push(ToolCall {
                        id: format!("gemini-{next_id}"),
                        name,
                        input,
                    });
                    next_id += 1;
                }
            }
        }
        Ok(StreamOutcome { text, tool_calls })
    }
}

/// The agentic loop for Gemini: stream → execute requested tools → feed
/// results back, until the model answers without tools (or the round budget
/// runs out). Mirrors [`super::claude::run_agent`] and
/// [`super::ollama::run_agent`]'s shape and the [`AiDelta`] sequence they
/// emit, but in Gemini's own `Content`/`Part` shape — see the module doc
/// comment for what that shape is and how it was confirmed.
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    provider: &GeminiProvider,
    api_key: &str,
    base_url: Option<&str>,
    model: &str,
    system: Option<&str>,
    history: &[ChatTurn],
    tools: &[ToolDef],
    executor: &dyn ToolExecutor,
    tx: &UnboundedSender<AiDelta>,
) -> AiResult<String> {
    // Gemini has no user-configurable endpoint (`base_url_setting_for` in
    // `ai_commands.rs` only names Ollama's), so this is always `None` from
    // the desktop layer — the parameter exists so tests can point `run_agent`
    // at a local server the way every other hermetic test here does.
    let base = base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/');
    let mut messages = contents(history);
    let mut transcript = String::new();

    for round in 0..MAX_AGENT_ROUNDS {
        // Last round: withhold tools so the model must conclude.
        let round_tools = if round + 1 == MAX_AGENT_ROUNDS {
            &[]
        } else {
            tools
        };
        let outcome = provider
            .stream_once(api_key, base, model, system, &messages, round_tools, tx)
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

        // The model's turn carries whatever it said plus a functionCall part
        // per call, in one `model`-role Content — this is what a real
        // response already looks like, so the continuation echoes it rather
        // than inventing a different shape.
        let mut model_parts: Vec<Value> = Vec::new();
        if !outcome.text.is_empty() {
            model_parts.push(json!({ "text": outcome.text }));
        }
        for call in &outcome.tool_calls {
            model_parts.push(json!({
                "functionCall": { "name": call.name, "args": call.input },
            }));
        }
        messages.push(json!({ "role": "model", "parts": model_parts }));

        // Every result from this round goes back together in one `user`-role
        // Content — there is no third "tool" role in Gemini's schema.
        let mut response_parts: Vec<Value> = Vec::new();
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
                summary: truncate_bytes(&content, 160),
            });
            response_parts.push(json!({
                "functionResponse": {
                    "name": call.name,
                    "response": { "content": truncate_bytes(&content, MAX_TOOL_RESULT_BYTES) },
                },
            }));
        }
        messages.push(json!({ "role": "user", "parts": response_parts }));
    }

    Ok(transcript)
}

#[async_trait::async_trait]
impl AiProvider for GeminiProvider {
    fn id(&self) -> &'static str {
        "gemini"
    }

    async fn stream_chat(
        &self,
        req: StreamRequest<'_>,
        tx: &UnboundedSender<AiDelta>,
    ) -> AiResult<String> {
        let api_key = req.api_key.ok_or(AiError::MissingKey("gemini"))?;
        let base = req
            .base_url
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/');
        let url = format!(
            "{base}/v1beta/models/{}:streamGenerateContent?alt=sse",
            req.model
        );

        let mut body = json!({ "contents": contents(req.messages) });
        if let Some(system) = req.system {
            body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
        }

        let response = self
            .client
            .post(&url)
            // Header rather than `?key=`: the URL appears in logs and errors.
            .header("x-goog-api-key", api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AiError::Api {
                provider: "gemini",
                status: status.as_u16(),
                body: truncate(&body, 500),
            });
        }

        let mut full = String::new();
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            buffer.extend_from_slice(&chunk?);
            // Frames arrive split across TCP reads, so lines are assembled
            // here rather than assuming one chunk is one event.
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let Some(payload) = line.trim_end().strip_prefix("data: ") else {
                    continue;
                };
                // The stream's terminator, when the server sends one.
                if payload == "[DONE]" {
                    continue;
                }
                let Ok(parsed) = serde_json::from_str::<Value>(payload) else {
                    // A malformed frame is not worth aborting a reply that is
                    // otherwise arriving correctly.
                    continue;
                };
                let text = text_from_chunk(&parsed);
                if !text.is_empty() {
                    full.push_str(&text);
                    let _ = tx.send(AiDelta::Text { text });
                }
            }
        }

        Ok(full)
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChatTurn;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// One-shot local server: captures the raw request, replies once. Keeps
    /// every test in this module offline.
    async fn one_shot(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 16384];
            let n = socket.read(&mut request).await.unwrap();
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
            String::from_utf8_lossy(&request[..n]).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    fn sse(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
        )
    }

    fn turns() -> Vec<ChatTurn> {
        vec![
            ChatTurn {
                role: "user".into(),
                content: "hi".into(),
            },
            ChatTurn {
                role: "assistant".into(),
                content: "hello".into(),
            },
            ChatTurn {
                role: "user".into(),
                content: "again".into(),
            },
        ]
    }

    #[test]
    fn the_assistant_role_is_renamed_to_model() {
        let mapped = contents(&turns());
        let roles: Vec<&str> = mapped.iter().map(|c| c["role"].as_str().unwrap()).collect();
        // Sending "assistant" is accepted by the API and quietly degrades the
        // reply, so this mapping is the whole point of the function.
        assert_eq!(roles, vec!["user", "model", "user"]);
        assert_eq!(mapped[0]["parts"][0]["text"], "hi");
    }

    #[test]
    fn a_chunk_with_no_text_yields_nothing_rather_than_failing() {
        // Real streams carry frames like these: a safety-only chunk, and a
        // finish frame with no parts.
        for chunk in [
            json!({ "candidates": [{ "safetyRatings": [] }] }),
            json!({ "candidates": [{ "content": { "parts": [] }, "finishReason": "STOP" }] }),
            json!({ "candidates": [] }),
            json!({ "usageMetadata": { "totalTokenCount": 7 } }),
        ] {
            assert_eq!(text_from_chunk(&chunk), "");
        }
    }

    #[test]
    fn multiple_parts_in_one_chunk_are_joined() {
        let chunk = json!({
            "candidates": [{ "content": { "parts": [{ "text": "a" }, { "text": "b" }] } }]
        });
        assert_eq!(text_from_chunk(&chunk), "ab");
    }

    #[tokio::test]
    async fn streams_text_and_returns_the_whole_reply() {
        let (base, server) = one_shot(Box::leak(
            sse(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}]}\n\n\
                 data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"lo\"}]}}]}\n\n\
                 data: [DONE]\n\n",
            )
            .into_boxed_str(),
        ))
        .await;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let reply = GeminiProvider::new()
            .stream_chat(
                StreamRequest {
                    model: "gemini-3.6-flash",
                    system: Some("be terse"),
                    messages: &turns(),
                    api_key: Some("test-key"),
                    base_url: Some(&base),
                },
                &tx,
            )
            .await
            .expect("stream succeeds");
        drop(tx);

        assert_eq!(reply, "Hello");
        let mut streamed = String::new();
        while let Some(AiDelta::Text { text }) = rx.recv().await {
            streamed.push_str(&text);
        }
        assert_eq!(streamed, "Hello", "deltas arrive as they stream");

        // Assert on what the server actually received, not on our intent.
        let raw = server.await.unwrap();
        assert!(
            raw.contains("x-goog-api-key: test-key"),
            "the key must travel as a header: {raw}"
        );
        assert!(
            !raw.contains("key=test-key"),
            "and must not be in the URL: {raw}"
        );
        assert!(raw.contains("streamGenerateContent?alt=sse"));
        assert!(raw.contains("gemini-3.6-flash"));
        assert!(
            raw.contains("systemInstruction"),
            "the system prompt is not a message: {raw}"
        );
        assert!(raw.contains("\"role\":\"model\""));
    }

    #[tokio::test]
    async fn a_malformed_frame_does_not_abort_the_reply() {
        let (base, _server) = one_shot(Box::leak(
            sse(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]}}]}\n\n\
                 data: {not json\n\n\
                 data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"!\"}]}}]}\n\n",
            )
            .into_boxed_str(),
        ))
        .await;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let reply = GeminiProvider::new()
            .stream_chat(
                StreamRequest {
                    model: "gemini-3.6-flash",
                    system: None,
                    messages: &turns(),
                    api_key: Some("k"),
                    base_url: Some(&base),
                },
                &tx,
            )
            .await
            .unwrap();
        assert_eq!(reply, "ok!", "the good frames still land");
    }

    #[tokio::test]
    async fn an_api_error_keeps_its_status_and_body() {
        let (base, _server) = one_shot(
            "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 26\r\nconnection: close\r\n\r\n\
             {\"error\":{\"code\":429}}\n",
        )
        .await;

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let error = GeminiProvider::new()
            .stream_chat(
                StreamRequest {
                    model: "gemini-3.6-flash",
                    system: None,
                    messages: &turns(),
                    api_key: Some("k"),
                    base_url: Some(&base),
                },
                &tx,
            )
            .await
            .unwrap_err();

        // 429 is the one a free-tier user will actually hit, so the status has
        // to survive to the UI rather than becoming a generic failure.
        match error {
            AiError::Api {
                provider, status, ..
            } => {
                assert_eq!(provider, "gemini");
                assert_eq!(status, 429);
            }
            other => panic!("expected an API error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_missing_key_never_reaches_the_network() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let error = GeminiProvider::new()
            .stream_chat(
                StreamRequest {
                    model: "gemini-3.6-flash",
                    system: None,
                    messages: &turns(),
                    api_key: None,
                    // Port 1 is closed: reaching the network would surface as
                    // a transport error, so MissingKey is positive proof.
                    base_url: Some("http://127.0.0.1:1"),
                },
                &tx,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, AiError::MissingKey("gemini")));
    }

    #[test]
    fn tool_defs_become_one_entry_with_every_declaration_inside() {
        let tools = vec![
            ToolDef {
                name: "read_file".into(),
                description: "Read a project file".into(),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
            ToolDef {
                name: "list_dir".into(),
                description: "List a directory".into(),
                input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
        ];
        let value = gemini_tools_json(&tools);
        let array = value.as_array().expect("tools is an array");
        assert_eq!(array.len(), 1, "one entry, not one per tool");
        let decls = array[0]["functionDeclarations"]
            .as_array()
            .expect("functionDeclarations array");
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0]["name"], "read_file");
        assert_eq!(decls[0]["description"], "Read a project file");
        assert_eq!(decls[0]["parameters"]["type"], "object");
        assert_eq!(decls[1]["name"], "list_dir");
    }

    #[test]
    fn parts_from_chunk_separates_text_from_a_function_call() {
        let chunk = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "Let me check. "},
                        {"functionCall": {"name": "read_file", "args": {"path": "a.rs"}}},
                    ]
                }
            }]
        });
        let (text, calls) = parts_from_chunk(&chunk);
        assert_eq!(text, "Let me check. ");
        assert_eq!(
            calls,
            vec![("read_file".to_string(), json!({"path": "a.rs"}))]
        );
    }

    #[test]
    fn parts_from_chunk_with_no_function_call_yields_an_empty_call_list() {
        let chunk = json!({
            "candidates": [{ "content": { "parts": [{ "text": "just text" }] } }]
        });
        let (text, calls) = parts_from_chunk(&chunk);
        assert_eq!(text, "just text");
        assert!(calls.is_empty());
    }

    /// Serves one canned SSE response per accepted connection, in order —
    /// the SSE equivalent of `ollama::tests::multi_round_ndjson`, needed
    /// because a tool-calling round trip is more than one HTTP request.
    async fn multi_round_sse(
        bodies: Vec<String>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut requests = Vec::new();
            for body in bodies {
                let response = sse(&body);
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

    /// The end-to-end case this module's tool support exists for: the model
    /// calls a tool, the executor runs it, the result is threaded back as a
    /// `functionResponse` in a `user`-role Content, and the model's second
    /// reply is what the caller receives.
    #[tokio::test]
    async fn run_agent_executes_a_tool_then_returns_the_final_reply() {
        let round1 = format!(
            "data: {}\n\n",
            json!({
                "candidates": [{
                    "content": {
                        "role": "model",
                        "parts": [{"functionCall": {"name": "read_file", "args": {"path": "src/main.rs"}}}],
                    }
                }]
            })
        );
        let round2 = format!(
            "data: {}\n\ndata: {}\n\n",
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "It says "}]}}]}),
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "hello."}]}}]}),
        );
        let (base, server) = multi_round_sse(vec![round1, round2]).await;

        let provider = GeminiProvider::new();
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
            "test-key",
            Some(&base),
            "gemini-3.6-flash",
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
                .any(|d| matches!(d, AiDelta::Text { text } if text == "It says ")),
            "expected the final reply to stream as Text, got {deltas:?}"
        );

        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2, "one HTTP request per agent round");

        let first = json_body_of(&requests[0]);
        assert_eq!(
            first["tools"][0]["functionDeclarations"][0]["name"], "read_file",
            "round 1 must offer the tool"
        );
        assert_eq!(
            first["systemInstruction"]["parts"][0]["text"],
            "You are a helpful assistant."
        );

        let second = json_body_of(&requests[1]);
        let contents = second["contents"].as_array().expect("contents array");
        let model_turn_has_call = contents.iter().any(|c| {
            c["role"] == "model"
                && c["parts"].as_array().is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|p| p["functionCall"]["name"] == "read_file")
                })
        });
        assert!(
            model_turn_has_call,
            "round 2 must echo the model's functionCall back, got {contents:#?}"
        );
        let carries_result = contents.iter().any(|c| {
            c["role"] == "user"
                && c["parts"].as_array().is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|p| p["functionResponse"]["response"]["content"] == "fn main() {}")
                })
        });
        assert!(
            carries_result,
            "round 2 must carry the executor's result back as a functionResponse, got {contents:#?}"
        );
    }

    /// A reply with no tool calls must not touch the executor or loop again.
    #[tokio::test]
    async fn a_plain_reply_takes_exactly_one_round() {
        let round1 = format!(
            "data: {}\n\n",
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "Hi there."}]}}]}),
        );
        let (base, server) = multi_round_sse(vec![round1]).await;

        struct PanicsIfCalled;
        #[async_trait::async_trait]
        impl ToolExecutor for PanicsIfCalled {
            async fn execute(&self, name: &str, _input: &Value) -> Result<String, String> {
                panic!("no tool call was made, but the executor was invoked with {name:?}");
            }
        }

        let provider = GeminiProvider::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let transcript = run_agent(
            &provider,
            "test-key",
            Some(&base),
            "gemini-3.6-flash",
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
}
