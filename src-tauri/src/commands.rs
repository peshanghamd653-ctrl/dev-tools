//! Tauri IPC commands. Thin layer: validate, call the kernel repository,
//! emit events. No business logic here.

use devos_kernel::types::{AppInfo, CommandDescriptor, JobInfo, KernelEvent, Project, Workspace};
use devos_kernel::{repo, KernelError};
use tauri::{AppHandle, State};

use crate::state::AppState;

fn err(e: KernelError) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, AppState>) -> Result<AppInfo, String> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        startup_ms: state.startup_ms,
        // From state, not re-derived — see `AppState::db_path`.
        db_path: state.db_path.clone(),
    })
}

// ---- Workspaces ----

#[tauri::command]
pub async fn workspaces_list(state: State<'_, AppState>) -> Result<Vec<Workspace>, String> {
    repo::list_workspaces(&state.kernel.pool).await.map_err(err)
}

#[tauri::command]
pub async fn workspace_create(
    state: State<'_, AppState>,
    name: String,
) -> Result<Workspace, String> {
    let workspace = repo::create_workspace(&state.kernel.pool, &name)
        .await
        .map_err(err)?;
    state.kernel.events.emit(KernelEvent::WorkspacesChanged);
    Ok(workspace)
}

#[tauri::command]
pub async fn workspace_rename(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<Workspace, String> {
    let workspace = repo::rename_workspace(&state.kernel.pool, &id, &name)
        .await
        .map_err(err)?;
    state.kernel.events.emit(KernelEvent::WorkspacesChanged);
    Ok(workspace)
}

#[tauri::command]
pub async fn workspace_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    repo::delete_workspace(&state.kernel.pool, &id)
        .await
        .map_err(err)?;
    state.kernel.events.emit(KernelEvent::WorkspacesChanged);
    Ok(())
}

// ---- Projects ----

#[tauri::command]
pub async fn projects_list(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<Vec<Project>, String> {
    repo::list_projects(&state.kernel.pool, &workspace_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn project_add(
    state: State<'_, AppState>,
    workspace_id: String,
    name: String,
    path: String,
) -> Result<Project, String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err(format!("path is not an existing directory: {path}"));
    }
    let project = repo::add_project(&state.kernel.pool, &workspace_id, &name, &path)
        .await
        .map_err(err)?;
    state
        .kernel
        .events
        .emit(KernelEvent::ProjectsChanged { workspace_id });
    Ok(project)
}

#[tauri::command]
pub async fn project_remove(
    state: State<'_, AppState>,
    id: String,
    workspace_id: String,
) -> Result<(), String> {
    repo::remove_project(&state.kernel.pool, &id)
        .await
        .map_err(err)?;
    state
        .kernel
        .events
        .emit(KernelEvent::ProjectsChanged { workspace_id });
    Ok(())
}

// ---- Settings ----

#[tauri::command]
pub async fn settings_get(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<String>, String> {
    repo::get_setting(&state.kernel.pool, &key)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn settings_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    repo::set_setting(&state.kernel.pool, &key, &value)
        .await
        .map_err(err)?;
    state
        .kernel
        .events
        .emit(KernelEvent::SettingsChanged { key });
    Ok(())
}

// ---- Kernel introspection ----

#[tauri::command]
pub async fn commands_list(state: State<'_, AppState>) -> Result<Vec<CommandDescriptor>, String> {
    Ok(state.kernel.commands.list())
}

#[tauri::command]
pub async fn jobs_recent(state: State<'_, AppState>) -> Result<Vec<JobInfo>, String> {
    state.kernel.jobs.list_recent(50).await.map_err(err)
}

// ---- Notifications ----

#[tauri::command]
pub async fn notifications_list(
    state: State<'_, AppState>,
) -> Result<Vec<devos_kernel::types::NotificationDto>, String> {
    repo::list_notifications(&state.kernel.pool, 50)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn notifications_unread_count(state: State<'_, AppState>) -> Result<i64, String> {
    repo::unread_notification_count(&state.kernel.pool)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn notification_mark_read(state: State<'_, AppState>, id: String) -> Result<(), String> {
    repo::mark_notification_read(&state.kernel.pool, &id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn notifications_mark_all_read(state: State<'_, AppState>) -> Result<(), String> {
    repo::mark_all_notifications_read(&state.kernel.pool)
        .await
        .map_err(err)
}
