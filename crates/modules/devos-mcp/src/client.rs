//! JSON-RPC 2.0 over MCP's stdio transport: one message per line, no
//! `Content-Length` header (unlike LSP) — writing means "append a newline and
//! flush," reading means "read a line and parse it."
//!
//! Generic over the transport so the protocol logic — request framing, id
//! correlation, response parsing, the `initialize` handshake, paginated
//! `tools/list` — is testable against an in-memory pipe ([`tokio::io::duplex`])
//! rather than only against a real child process. [`crate::process`] is the
//! thin adapter that points this at one.

use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::{McpError, McpResult, McpTool};

/// How long a single request is allowed to take. Generous — some MCP
/// servers spend real time on their first import — but not unbounded: a hung
/// child process must not hang the "connect and list tools" UI forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// The MCP protocol version this client claims in `initialize`. Most server
/// implementations are lenient about an exact match.
const PROTOCOL_VERSION: &str = "2025-06-18";

pub struct RpcConn<R, W> {
    reader: BufReader<R>,
    writer: W,
    next_id: i64,
}

impl<R, W> RpcConn<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
        }
    }

    async fn call(&mut self, method: &str, params: Value) -> McpResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.write_line(&request).await?;

        loop {
            let line = self.read_line().await?;
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                return Err(McpError::Protocol(format!(
                    "server sent a non-JSON line: {line}"
                )));
            };
            // A message with no "id" is a notification, not this call's
            // response — keep reading. A message with a *different* id
            // belongs to some other in-flight call; this client never has
            // more than one, but skipping rather than erroring costs nothing.
            let Some(msg_id) = message.get("id") else {
                continue;
            };
            if msg_id != &Value::from(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(McpError::Rpc(error.to_string()));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// A notification: no id, no response expected. `notifications/initialized`
    /// is the only one this client ever sends.
    async fn notify(&mut self, method: &str, params: Value) -> McpResult<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_line(&notification).await
    }

    async fn write_line(&mut self, value: &Value) -> McpResult<()> {
        let mut line =
            serde_json::to_string(value).map_err(|e| McpError::Protocol(e.to_string()))?;
        line.push('\n');
        tokio::time::timeout(REQUEST_TIMEOUT, self.writer.write_all(line.as_bytes()))
            .await
            .map_err(|_| McpError::Timeout)??;
        tokio::time::timeout(REQUEST_TIMEOUT, self.writer.flush())
            .await
            .map_err(|_| McpError::Timeout)??;
        Ok(())
    }

    async fn read_line(&mut self) -> McpResult<String> {
        let mut line = String::new();
        let bytes = tokio::time::timeout(REQUEST_TIMEOUT, self.reader.read_line(&mut line))
            .await
            .map_err(|_| McpError::Timeout)??;
        if bytes == 0 {
            return Err(McpError::Protocol(
                "server closed the connection".to_string(),
            ));
        }
        Ok(line)
    }

    /// `initialize`, then the `notifications/initialized` acknowledgement the
    /// spec requires before any other request. Returns the server's
    /// advertised name for display.
    pub async fn initialize(&mut self) -> McpResult<String> {
        let result = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "DevOS", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result
            .get("serverInfo")
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("unnamed server")
            .to_string())
    }

    /// `tools/list`, following `nextCursor` until it stops appearing. Most
    /// stdio servers never paginate at all; honoring the field costs nothing
    /// when they don't.
    pub async fn list_tools(&mut self) -> McpResult<Vec<McpTool>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut params = json!({});
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }
            let result = self.call("tools/list", params).await?;
            let page = result
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for tool in page {
                tools.push(McpTool {
                    name: tool
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    description: tool
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    input_schema: tool
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                });
            }
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::split;

    /// A fake server on the other end of a `duplex` pipe: reads one line,
    /// hands back a canned response line. For a response the client's
    /// `initialize()` treats as an *error* — it returns before sending
    /// anything further, so there is nothing more for this to read.
    async fn respond_once(
        server_read: impl AsyncRead + Unpin,
        mut server_write: impl AsyncWrite + Unpin,
        response: &str,
    ) -> String {
        let mut reader = BufReader::new(server_read);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        server_write
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        server_write.flush().await.unwrap();
        line
    }

    /// The same shape, for a *successful* `initialize()` reply: reads the
    /// request, hands back a canned response, then reads (and discards) the
    /// `notifications/initialized` notification the client sends right
    /// after a successful handshake. That last read matters — without it,
    /// this function returns and its `server_read`/`server_write` arguments
    /// drop as soon as the caller's task ends, closing the pipe before the
    /// client's notification write can land and turning a perfectly
    /// successful handshake into a broken-pipe error.
    async fn handshake(
        server_read: impl AsyncRead + Unpin,
        mut server_write: impl AsyncWrite + Unpin,
        response: &str,
    ) -> String {
        let mut reader = BufReader::new(server_read);
        let mut request = String::new();
        reader.read_line(&mut request).await.unwrap();
        server_write
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        server_write.flush().await.unwrap();
        let mut notification = String::new();
        reader.read_line(&mut notification).await.unwrap();
        request
    }

    #[tokio::test]
    async fn initialize_sends_the_handshake_and_reads_the_server_name() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (client_read, client_write) = split(client_side);
        let (server_read, server_write) = split(server_side);
        let mut conn = RpcConn::new(client_read, client_write);

        let server = tokio::spawn(async move {
            let request = handshake(
                server_read,
                server_write,
                r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"weather-server"}}}"#,
            )
            .await;
            serde_json::from_str::<Value>(&request).unwrap()
        });

        let name = conn.initialize().await.unwrap();
        let request = server.await.unwrap();

        assert_eq!(name, "weather-server");
        assert_eq!(request["method"], "initialize");
        assert_eq!(request["params"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(request["params"]["clientInfo"]["name"], "DevOS");
    }

    #[tokio::test]
    async fn a_missing_server_name_falls_back_rather_than_failing() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (client_read, client_write) = split(client_side);
        let (server_read, server_write) = split(server_side);
        let mut conn = RpcConn::new(client_read, client_write);

        tokio::spawn(async move {
            handshake(
                server_read,
                server_write,
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            )
            .await;
        });

        assert_eq!(conn.initialize().await.unwrap(), "unnamed server");
    }

    #[tokio::test]
    async fn an_rpc_error_response_becomes_an_error_not_a_panic() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (client_read, client_write) = split(client_side);
        let (server_read, server_write) = split(server_side);
        let mut conn = RpcConn::new(client_read, client_write);

        tokio::spawn(async move {
            respond_once(
                server_read,
                server_write,
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not found"}}"#,
            )
            .await;
        });

        let error = conn.initialize().await.unwrap_err();
        assert!(matches!(error, McpError::Rpc(_)), "{error:?}");
        assert!(error.to_string().contains("method not found"));
    }

    #[tokio::test]
    async fn list_tools_follows_a_cursor_across_two_pages() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (client_read, client_write) = split(client_side);
        let (server_read, server_write) = split(server_side);
        let mut conn = RpcConn::new(client_read, client_write);

        tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut writer = server_write;

            let mut first = String::new();
            reader.read_line(&mut first).await.unwrap();
            writer
                .write_all(
                    br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"search"}],"nextCursor":"page2"}}
"#,
                )
                .await
                .unwrap();

            let mut second = String::new();
            reader.read_line(&mut second).await.unwrap();
            writer
                .write_all(
                    br#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"fetch","description":"HTTP GET","inputSchema":{"type":"object"}}]}}
"#,
                )
                .await
                .unwrap();
            writer.flush().await.unwrap();

            (first, second)
        });

        let tools = conn.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2, "{tools:?}");
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].description, None);
        assert_eq!(tools[1].name, "fetch");
        assert_eq!(tools[1].description.as_deref(), Some("HTTP GET"));
        assert_eq!(tools[1].input_schema, json!({"type": "object"}));
    }

    #[tokio::test]
    async fn a_non_json_line_is_a_protocol_error_not_a_crash() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (client_read, client_write) = split(client_side);
        let (_server_read, mut server_write) = split(server_side);
        let mut conn = RpcConn::new(client_read, client_write);

        tokio::spawn(async move {
            server_write.write_all(b"not json at all\n").await.unwrap();
            server_write.flush().await.unwrap();
        });

        let error = conn.initialize().await.unwrap_err();
        assert!(matches!(error, McpError::Protocol(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_closed_connection_before_any_reply_is_a_protocol_error() {
        let (client_side, server_side) = tokio::io::duplex(4096);
        let (client_read, client_write) = split(client_side);
        let (server_read, server_write) = split(server_side);
        let mut conn = RpcConn::new(client_read, client_write);

        // Reads the request (so the client's write succeeds) and then just
        // disappears — the failure this test wants is on the *read* side,
        // not a broken pipe on the write the client never gets a chance to
        // make.
        tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut request = String::new();
            reader.read_line(&mut request).await.unwrap();
            drop(server_write);
        });

        let error = conn.initialize().await.unwrap_err();
        assert!(matches!(error, McpError::Protocol(_)), "{error:?}");
    }
}
