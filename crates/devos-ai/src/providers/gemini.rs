//! Gemini adapter: Google's Generative Language API with SSE streaming.
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
//! Tool calling is **not** implemented here. Gemini has function calling, but
//! the agent loop in `claude.rs` is written against Anthropic's `tool_use`
//! blocks, and a second loop is a bigger change than adding a provider. The
//! desktop layer gates tools on `provider == "claude"`, so a Gemini
//! conversation streams plain chat rather than silently dropping tool grants.

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use super::{AiError, AiProvider, AiResult, StreamRequest};
use crate::types::AiDelta;

/// Overridable so tests can point at a local server, and so a user behind a
/// proxy has somewhere to aim. `v1beta` is where `generateContent` lives.
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

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
}
