//! Environment variable manager IPC. No `AppState` needed — every operation
//! here is plain filesystem I/O against a project's own `.env` files, not
//! anything that touches the kernel's pool.

use std::path::Path;

use devos_envfile::EnvEntry;

#[tauri::command]
pub async fn env_file_list(project_path: String) -> Result<Vec<String>, String> {
    Ok(devos_envfile::list_env_files(Path::new(&project_path)))
}

#[tauri::command]
pub async fn env_file_read(
    project_path: String,
    file_name: String,
) -> Result<Vec<EnvEntry>, String> {
    let path =
        devos_envfile::resolve(Path::new(&project_path), &file_name).map_err(|e| e.to_string())?;
    devos_envfile::read(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn env_file_set(
    project_path: String,
    file_name: String,
    key: String,
    value: String,
) -> Result<(), String> {
    let path =
        devos_envfile::resolve(Path::new(&project_path), &file_name).map_err(|e| e.to_string())?;
    devos_envfile::set(&path, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn env_file_delete_key(
    project_path: String,
    file_name: String,
    key: String,
) -> Result<(), String> {
    let path =
        devos_envfile::resolve(Path::new(&project_path), &file_name).map_err(|e| e.to_string())?;
    devos_envfile::remove(&path, &key).map_err(|e| e.to_string())
}
