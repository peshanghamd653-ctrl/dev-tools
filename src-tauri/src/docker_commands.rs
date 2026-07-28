//! Docker IPC commands: thin pass-throughs; availability errors carry a
//! recognizable prefix so the UI can show the "Docker isn't running" state.

use devos_docker::{DockerContainer, DockerError, DockerImage};

fn err(e: DockerError) -> String {
    match e {
        DockerError::Unavailable(detail) => format!("unavailable: {detail}"),
        DockerError::Api(detail) => detail,
    }
}

#[tauri::command]
pub async fn docker_ping() -> Result<String, String> {
    devos_docker::ping().await.map_err(err)
}

#[tauri::command]
pub async fn docker_containers() -> Result<Vec<DockerContainer>, String> {
    devos_docker::list_containers().await.map_err(err)
}

#[tauri::command]
pub async fn docker_images() -> Result<Vec<DockerImage>, String> {
    devos_docker::list_images().await.map_err(err)
}

#[tauri::command]
pub async fn docker_start(id: String) -> Result<(), String> {
    devos_docker::start_container(&id).await.map_err(err)
}

#[tauri::command]
pub async fn docker_stop(id: String) -> Result<(), String> {
    devos_docker::stop_container(&id).await.map_err(err)
}

#[tauri::command]
pub async fn docker_restart(id: String) -> Result<(), String> {
    devos_docker::restart_container(&id).await.map_err(err)
}

#[tauri::command]
pub async fn docker_logs(id: String) -> Result<String, String> {
    devos_docker::container_logs(&id, 200).await.map_err(err)
}
