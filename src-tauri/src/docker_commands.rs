//! Docker IPC commands: thin pass-throughs; availability errors carry a
//! recognizable prefix so the UI can show the "Docker isn't running" state.

use std::path::Path;

use devos_docker::{ComposeError, ComposeStatus, DockerContainer, DockerError, DockerImage};

fn err(e: DockerError) -> String {
    match e {
        DockerError::Unavailable(detail) => format!("unavailable: {detail}"),
        DockerError::Api(detail) => detail,
    }
}

/// Same `"unavailable: "` prefix `err` gives `DockerError::Unavailable` —
/// `isUnavailable` on the frontend checks for exactly that substring, and a
/// missing `docker compose` is the same "nothing to show yet" UI state as a
/// stopped daemon, not a distinct one.
fn compose_err(e: ComposeError) -> String {
    match e {
        ComposeError::NotInstalled => "unavailable: docker compose is not installed".into(),
        other => other.to_string(),
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

/// `None` means no compose file in this project — not an error, just
/// nothing for the frontend to show a Compose section for.
#[tauri::command]
pub async fn docker_compose_detect(project_path: String) -> Result<Option<String>, String> {
    Ok(
        devos_docker::compose::find_compose_file(Path::new(&project_path))
            .map(|path| path.display().to_string()),
    )
}

#[tauri::command]
pub async fn docker_compose_status(compose_file: String) -> Result<ComposeStatus, String> {
    devos_docker::compose_status(Path::new(&compose_file))
        .await
        .map_err(compose_err)
}

#[tauri::command]
pub async fn docker_compose_up(
    compose_file: String,
    service: Option<String>,
) -> Result<(), String> {
    devos_docker::compose::up(Path::new(&compose_file), service.as_deref())
        .await
        .map_err(compose_err)
}

#[tauri::command]
pub async fn docker_compose_down(compose_file: String) -> Result<(), String> {
    devos_docker::compose::down(Path::new(&compose_file))
        .await
        .map_err(compose_err)
}

#[tauri::command]
pub async fn docker_compose_restart(
    compose_file: String,
    service: Option<String>,
) -> Result<(), String> {
    devos_docker::compose::restart(Path::new(&compose_file), service.as_deref())
        .await
        .map_err(compose_err)
}
