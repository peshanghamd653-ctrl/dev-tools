//! Screenshot → GitHub issue IPC commands.
//!
//! The token stops here. `devos_issue` takes it as a plain parameter, so the
//! secret store is unreachable from the module crate — which is also what
//! lets every one of that crate's tests run offline.

use std::path::PathBuf;

use devos_issue::{CapturedShot, CreatedIssue, IssueError, IssueTarget, DEFAULT_BASE_URL};
use tauri::State;

use crate::state::AppState;

/// Name the GitHub token is stored under in the secret store.
const TOKEN_SECRET: &str = "github_token";

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
        .ok_or_else(|| IssueError::NotConfigured.to_string())
}

/// Whether a token is stored, so the UI can offer the "add a token" state
/// rather than presenting a missing token as a failed request.
#[tauri::command]
pub async fn issue_configured(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(stored_token(&state).await?.is_some())
}

#[tauri::command]
pub async fn issue_capture(state: State<'_, AppState>) -> Result<CapturedShot, String> {
    // From state, not `app_data_dir()` — screenshots follow `DEVOS_DATA_DIR`
    // like everything else this app writes, and `setup` grants the asset
    // protocol that same directory so the annotator can load the result.
    let app_data = state.data_dir.clone();
    // Capture reads from the display driver and then encodes several
    // megabytes of PNG. On the async runtime that would stall every other IPC
    // call for its duration, so it goes to a blocking thread.
    tokio::task::spawn_blocking(move || devos_issue::capture_primary(&app_data))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn issue_targets(project_path: String) -> Result<Vec<IssueTarget>, String> {
    devos_issue::github_targets(&PathBuf::from(project_path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn issue_create(
    state: State<'_, AppState>,
    owner: String,
    name: String,
    title: String,
    body: String,
) -> Result<CreatedIssue, String> {
    let token = require_token(&state).await?;
    devos_issue::create_issue(&token, DEFAULT_BASE_URL, &owner, &name, &title, &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn issue_copy_image(path: String) -> Result<(), String> {
    // The clipboard is a synchronous OS API and the decode is CPU work on a
    // full-screen image, so this joins the capture off the async runtime.
    tokio::task::spawn_blocking(move || devos_issue::copy_image(&PathBuf::from(path)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
