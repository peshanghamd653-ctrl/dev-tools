//! Git IPC commands. Thin: resolve the path, call the module, map errors.

use std::path::PathBuf;

use devos_git::{ConflictSides, GitBranch, GitCommit, GitStatus, RebaseStatus};

fn repo(path: &str) -> PathBuf {
    PathBuf::from(path)
}

fn err(e: devos_git::GitError) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn git_status(path: String) -> Result<GitStatus, String> {
    let (info, entries) = devos_git::status(&repo(&path)).await.map_err(err)?;
    Ok(GitStatus { info, entries })
}

#[tauri::command]
pub async fn git_stage(path: String, files: Vec<String>) -> Result<(), String> {
    devos_git::stage(&repo(&path), &files).await.map_err(err)
}

#[tauri::command]
pub async fn git_unstage(path: String, files: Vec<String>) -> Result<(), String> {
    devos_git::unstage(&repo(&path), &files).await.map_err(err)
}

#[tauri::command]
pub async fn git_discard(path: String, file: String, untracked: bool) -> Result<(), String> {
    devos_git::discard(&repo(&path), &file, untracked)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_commit(path: String, message: String) -> Result<String, String> {
    devos_git::commit(&repo(&path), &message).await.map_err(err)
}

#[tauri::command]
pub async fn git_log(path: String, limit: u32) -> Result<Vec<GitCommit>, String> {
    devos_git::log(&repo(&path), limit).await.map_err(err)
}

#[tauri::command]
pub async fn git_branches(path: String) -> Result<Vec<GitBranch>, String> {
    devos_git::branches(&repo(&path)).await.map_err(err)
}

#[tauri::command]
pub async fn git_switch(path: String, branch: String, create: bool) -> Result<(), String> {
    devos_git::switch(&repo(&path), &branch, create)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_diff(
    path: String,
    file: String,
    staged: bool,
    untracked: bool,
) -> Result<String, String> {
    devos_git::diff_file(&repo(&path), &file, staged, untracked)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_push(path: String) -> Result<String, String> {
    devos_git::push(&repo(&path)).await.map_err(err)
}

#[tauri::command]
pub async fn git_pull(path: String) -> Result<String, String> {
    devos_git::pull(&repo(&path)).await.map_err(err)
}

#[tauri::command]
pub async fn git_conflict_sides(path: String, file: String) -> Result<ConflictSides, String> {
    devos_git::conflict_sides(&repo(&path), &file)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_resolve_ours(path: String, file: String) -> Result<(), String> {
    devos_git::resolve_ours(&repo(&path), &file)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_resolve_theirs(path: String, file: String) -> Result<(), String> {
    devos_git::resolve_theirs(&repo(&path), &file)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_resolve_with_content(
    path: String,
    file: String,
    content: String,
) -> Result<(), String> {
    devos_git::resolve_with_content(&repo(&path), &file, &content)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_rebase_start(path: String, onto: String) -> Result<(), String> {
    devos_git::rebase_start(&repo(&path), &onto)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn git_rebase_continue(path: String) -> Result<(), String> {
    devos_git::rebase_continue(&repo(&path)).await.map_err(err)
}

#[tauri::command]
pub async fn git_rebase_abort(path: String) -> Result<(), String> {
    devos_git::rebase_abort(&repo(&path)).await.map_err(err)
}

#[tauri::command]
pub async fn git_rebase_skip(path: String) -> Result<(), String> {
    devos_git::rebase_skip(&repo(&path)).await.map_err(err)
}

#[tauri::command]
pub async fn git_rebase_status(path: String) -> Result<RebaseStatus, String> {
    Ok(devos_git::rebase_status(&repo(&path)).await)
}
