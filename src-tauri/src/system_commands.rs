//! System-metrics IPC command. The probe is long-lived (see
//! `devos_system::SystemProbe`) so CPU usage always has a previous sample to
//! measure against.

use devos_system::SystemSnapshot;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn system_snapshot(state: State<'_, AppState>) -> Result<SystemSnapshot, String> {
    let probe = state.system.clone();
    // Refreshing sysinfo is synchronous, and may sleep out the remainder of
    // the minimum CPU sampling interval, so it stays off the async workers.
    tokio::task::spawn_blocking(move || probe.snapshot())
        .await
        .map_err(|e| e.to_string())
}
