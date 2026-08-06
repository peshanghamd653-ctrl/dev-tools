//! Uptime-monitor IPC commands. The background scheduler started in `lib.rs`
//! does the periodic checking; these are the user-driven edges.

use devos_monitor::{Monitor, MonitorCheck, MonitorStatus};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn monitors_list(state: State<'_, AppState>) -> Result<Vec<MonitorStatus>, String> {
    devos_monitor::list_status(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_create(
    state: State<'_, AppState>,
    name: String,
    url: String,
    interval_secs: i64,
) -> Result<Monitor, String> {
    devos_monitor::create_monitor(&state.kernel.pool, &name, &url, interval_secs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    devos_monitor::delete_monitor(&state.kernel.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_toggle(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<Monitor, String> {
    devos_monitor::set_enabled(&state.kernel.pool, &id, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_check_now(
    state: State<'_, AppState>,
    id: String,
) -> Result<MonitorCheck, String> {
    devos_monitor::check_now(&state.kernel, &id)
        .await
        .map_err(|e| e.to_string())
}
