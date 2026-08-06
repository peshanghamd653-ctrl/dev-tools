//! Vercel deployment IPC commands. Read-only: DevOS shows what shipped, it
//! never ships anything.
//!
//! The token stops here. `devos_deploy` takes it as a plain parameter, so the
//! secret store is unreachable from the module crate — which is also what
//! lets every one of that crate's tests run offline.

use devos_deploy::{DeployError, DeployProject, Deployment, DEFAULT_BASE_URL};
use tauri::State;

use crate::state::AppState;

/// Name the Vercel token is stored under in the secret store.
const TOKEN_SECRET: &str = "vercel_token";

/// The stored token, treating a blank value as absent.
async fn stored_token(state: &AppState) -> Result<Option<String>, String> {
    Ok(state
        .secrets
        .get(TOKEN_SECRET)
        .await
        .map_err(|e| e.to_string())?
        .filter(|value| !value.trim().is_empty()))
}

async fn require_token(state: &AppState) -> Result<String, String> {
    stored_token(state)
        .await?
        .ok_or_else(|| DeployError::NotConfigured.to_string())
}

/// Whether a token is stored, so the UI can offer the "add a token" state
/// rather than presenting a missing token as a failed request.
#[tauri::command]
pub async fn deploy_configured(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(stored_token(&state).await?.is_some())
}

#[tauri::command]
pub async fn deploy_projects(state: State<'_, AppState>) -> Result<Vec<DeployProject>, String> {
    let token = require_token(&state).await?;
    devos_deploy::list_projects(&token, DEFAULT_BASE_URL)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn deploy_list(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<Deployment>, String> {
    let token = require_token(&state).await?;
    devos_deploy::list_deployments(&token, DEFAULT_BASE_URL, &project_id)
        .await
        .map_err(|e| e.to_string())
}
