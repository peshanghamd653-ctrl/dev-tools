//! Adapts a real child process's stdio into [`crate::client::RpcConn`].
//!
//! One function, one job: spawn `command args...`, run the MCP handshake,
//! list its tools, then terminate the process — whether that succeeded or
//! not. See the module doc comment on why this does not keep the process
//! running afterward.

use std::process::Stdio;

use tokio::process::Command;

use crate::client::RpcConn;
use crate::{McpError, McpResult, McpTool};

/// `(server name, tools)`. The name comes from the server's own `initialize`
/// response, not from the saved [`crate::McpServer::name`] the user chose —
/// the two can differ, and showing the server's self-reported identity next
/// to the user's label is a sanity check worth keeping visible.
pub async fn discover_tools(command: &str, args: &[String]) -> McpResult<(String, Vec<McpTool>)> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // A server's diagnostic chatter on stderr is not part of the
        // protocol and must never be parsed as one; inheriting it would also
        // leak into DevOS's own log stream. Discarded, not captured — there
        // is no UI surface for it yet.
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| McpError::Spawn(e.to_string()))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| McpError::Spawn("child process has no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| McpError::Spawn("child process has no stdout".into()))?;

    let result = {
        let mut conn = RpcConn::new(stdout, stdin);
        async {
            let server_name = conn.initialize().await?;
            let tools = conn.list_tools().await?;
            Ok::<_, McpError>((server_name, tools))
        }
        .await
    };

    // Best-effort cleanup either way: a discovery failure must not leave an
    // orphaned process behind for the user to notice in Task Manager later.
    let _ = child.start_kill();
    let _ = child.wait().await;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No fixture MCP server to spawn in a unit test, so this exercises the
    /// one path that doesn't need one: a command that fails to launch at all.
    /// The stdio-framing and handshake logic itself is covered against a
    /// real (in-memory) transport in `client::tests`.
    #[tokio::test]
    async fn a_command_that_does_not_exist_is_a_spawn_error_not_a_panic() {
        let error = discover_tools("devos-mcp-test-binary-that-does-not-exist", &[])
            .await
            .unwrap_err();
        assert!(matches!(error, McpError::Spawn(_)), "{error:?}");
    }
}
