//! API-client IPC commands. `api_send` records history automatically and
//! resolves `{{VAR}}` placeholders against the active environment, if any.

use devos_api::{
    ApiEnvVar, ApiEnvironment, ApiHistoryEntry, ApiRequestSpec, ApiResponse, SavedRequest,
};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn api_send(
    state: State<'_, AppState>,
    spec: ApiRequestSpec,
) -> Result<ApiResponse, String> {
    let active = devos_api::active_environment(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())?;
    let resolved = match &active {
        Some(env) => devos_api::substitute_spec(&spec, &env.vars),
        None => spec.clone(),
    };

    let response = devos_api::send_request(&resolved)
        .await
        .map_err(|e| e.to_string())?;

    // The *original*, templated method/url go into history — never the
    // resolved ones. A URL can carry a secret as a query parameter
    // (`?api_key={{TOKEN}}` is a common, legitimate shape), and history has
    // no expiry beyond its 100-entry cap and no per-entry delete. Recording
    // `{{API_URL}}/users` instead of the literal resolved host is a
    // deliberate trade of precision for never writing a resolved secret
    // into a persisted, undeleteable-by-default table — the same rule
    // `audit_log` already follows for tool calls (record the action, never
    // a value that could be a credential).
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
pub async fn api_environments(state: State<'_, AppState>) -> Result<Vec<ApiEnvironment>, String> {
    devos_api::list_environments(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_environment_create(
    state: State<'_, AppState>,
    name: String,
) -> Result<ApiEnvironment, String> {
    devos_api::create_environment(&state.kernel.pool, &name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_environment_update(
    state: State<'_, AppState>,
    id: String,
    name: String,
    vars: Vec<ApiEnvVar>,
) -> Result<ApiEnvironment, String> {
    devos_api::update_environment(&state.kernel.pool, &id, &name, &vars)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_environment_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    devos_api::delete_environment(&state.kernel.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_environment_set_active(
    state: State<'_, AppState>,
    id: Option<String>,
) -> Result<(), String> {
    devos_api::set_active_environment(&state.kernel.pool, id.as_deref())
        .await
        .map_err(|e| e.to_string())
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
