//! MCP server IPC commands: saved configurations, plus a one-shot "connect
//! and list tools" discovery call. Deliberately does not expose a way to
//! *invoke* a discovered tool — see `devos_mcp`'s module doc comment for why.

use devos_mcp::{McpServer, McpTool};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServer>, String> {
    devos_mcp::list_servers(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_server_create(
    state: State<'_, AppState>,
    name: String,
    command: String,
    args: Vec<String>,
) -> Result<McpServer, String> {
    devos_mcp::create_server(&state.kernel.pool, &name, &command, &args)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_server_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    devos_mcp::delete_server(&state.kernel.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Spawn the server's launch command, perform the MCP handshake, list its
/// tools, then terminate the process — a health check and capability
/// preview, not a standing connection. `(server_name, tools)`.
#[tauri::command]
pub async fn mcp_discover_tools(
    command: String,
    args: Vec<String>,
) -> Result<(String, Vec<McpTool>), String> {
    devos_mcp::discover_tools(&command, &args)
        .await
        .map_err(|e| e.to_string())
}
