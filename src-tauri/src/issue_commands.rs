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

/// A path that is genuinely one of the user's registered projects.
///
/// `issue_targets` runs `git` with its working directory set to whatever it is
/// given, so an unvalidated path is a shell-out anywhere on the machine. The
/// UI only ever passes a path it got from the projects list, so constraining
/// it to that list costs nothing and closes the gap. Matching is by
/// canonicalized path so a differently-spelled route to the same directory
/// still resolves — the point is *which directory*, not which spelling.
async fn registered_project(state: &AppState, path: &str) -> Result<PathBuf, String> {
    let candidate = std::fs::canonicalize(path).map_err(|_| "not a project path".to_string())?;
    let workspaces = devos_kernel::repo::list_workspaces(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())?;
    for workspace in workspaces {
        let projects = devos_kernel::repo::list_projects(&state.kernel.pool, &workspace.id)
            .await
            .map_err(|e| e.to_string())?;
        if projects
            .iter()
            .any(|p| std::fs::canonicalize(&p.path).is_ok_and(|known| known == candidate))
        {
            return Ok(candidate);
        }
    }
    Err("not a project path".into())
}

#[tauri::command]
pub async fn issue_targets(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<Vec<IssueTarget>, String> {
    let path = registered_project(&state, &project_path).await?;
    devos_issue::github_targets(&path)
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

/// A capture this app took, and nothing else.
///
/// `copy_image` hands the bytes to an image decoder — a parser running on
/// whatever file it is pointed at — so the path is constrained to the
/// screenshots directory rather than trusted. Canonicalized on both sides so
/// `..`, a symlink, or a differently-spelled route out of the folder all fail
/// the containment check rather than the string comparison.
fn shot_in_screenshots(state: &AppState, path: &str) -> Result<PathBuf, String> {
    let refused = || "not a DevOS screenshot".to_string();
    let root = std::fs::canonicalize(state.data_dir.join("screenshots")).map_err(|_| refused())?;
    let candidate = std::fs::canonicalize(path).map_err(|_| refused())?;
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Err(refused());
    }
    Ok(candidate)
}

#[tauri::command]
pub async fn issue_copy_image(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let path = shot_in_screenshots(&state, &path)?;
    // The clipboard is a synchronous OS API and the decode is CPU work on a
    // full-screen image, so this joins the capture off the async runtime.
    tokio::task::spawn_blocking(move || devos_issue::copy_image(&path))
        .await
        .map_err(|e| e.to_string())?
        // The path is deliberately absent from this message: it is the app's
        // own directory, and echoing it back only helps someone probing.
        .map_err(|_| "could not copy the screenshot to the clipboard".to_string())
}
