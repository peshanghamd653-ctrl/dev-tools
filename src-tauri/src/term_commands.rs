//! Terminal IPC commands: bridge pty session streams onto Tauri channels.

use devos_kernel::repo;
use devos_terminal::{CreateSessionOptions, TermEvent, TermSessionInfo};
use tauri::ipc::Channel;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn term_create(
    state: State<'_, AppState>,
    on_output: Channel<TermEvent>,
    shell: Option<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<TermSessionInfo, String> {
    let shell_setting = repo::get_setting(&state.kernel.pool, "terminal.shell")
        .await
        .map_err(|e| e.to_string())?;
    let integration_setting = repo::get_setting(&state.kernel.pool, "terminal.integration")
        .await
        .map_err(|e| e.to_string())?;

    let handle = state
        .terminal
        .create(CreateSessionOptions {
            shell: shell.or(shell_setting),
            cwd,
            cols,
            rows,
            shell_integration: integration_setting.as_deref() != Some("off"),
        })
        .map_err(|e| e.to_string())?;

    let info = handle.info.clone();
    let mut events = handle.events;
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            if on_output.send(event).is_err() {
                break;
            }
        }
    });

    Ok(info)
}

#[tauri::command]
pub async fn term_write(
    state: State<'_, AppState>,
    id: String,
    data: String,
) -> Result<(), String> {
    state
        .terminal
        .write(&id, data.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn term_resize(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .terminal
        .resize(&id, cols, rows)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn term_kill(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.terminal.kill(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn term_list(state: State<'_, AppState>) -> Result<Vec<TermSessionInfo>, String> {
    Ok(state.terminal.list())
}

/// Recent output of a session, ANSI-stripped (for AI diagnosis).
#[tauri::command]
pub async fn term_tail(state: State<'_, AppState>, id: String) -> Result<String, String> {
    state.terminal.tail(&id).map_err(|e| e.to_string())
}
