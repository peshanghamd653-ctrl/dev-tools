//! API-client IPC commands. `api_send` records history automatically.

use devos_api::{ApiHistoryEntry, ApiRequestSpec, ApiResponse, SavedRequest};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn api_send(
    state: State<'_, AppState>,
    spec: ApiRequestSpec,
) -> Result<ApiResponse, String> {
    let response = devos_api::send_request(&spec)
        .await
        .map_err(|e| e.to_string())?;
    let _ = devos_api::record_history(
        &state.kernel.pool,
        &spec.method,
        &spec.url,
        response.status,
        response.duration_ms,
    )
    .await;
    Ok(response)
}

#[tauri::command]
pub async fn api_save(
    state: State<'_, AppState>,
    name: String,
    collection: String,
    spec: ApiRequestSpec,
) -> Result<SavedRequest, String> {
    devos_api::save_request(&state.kernel.pool, &name, &collection, &spec)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_requests(state: State<'_, AppState>) -> Result<Vec<SavedRequest>, String> {
    devos_api::list_requests(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_request_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    devos_api::delete_request(&state.kernel.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_history(state: State<'_, AppState>) -> Result<Vec<ApiHistoryEntry>, String> {
    devos_api::list_history(&state.kernel.pool, 50)
        .await
        .map_err(|e| e.to_string())
}
