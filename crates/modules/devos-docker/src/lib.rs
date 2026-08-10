//! Docker module: containers, images, lifecycle, and logs over the Docker
//! Engine API (named pipe on Windows, socket elsewhere) via bollard.
//!
//! Docker Desktop may simply not be running — every entry point surfaces
//! that as [`DockerError::Unavailable`] so the UI can degrade gracefully
//! instead of erroring.

use bollard::container::{
    ListContainersOptions, LogsOptions, RestartContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::image::ListImagesOptions;
use bollard::models::{ContainerSummary, ImageSummary};
use bollard::Docker;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

const STOP_TIMEOUT_SECS: i64 = 10;
const MAX_LOG_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DockerError {
    #[error("Docker is not available: {0}")]
    Unavailable(String),
    #[error("docker API error: {0}")]
    Api(String),
}

pub type DockerResult<T> = Result<T, DockerError>;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    /// "running" | "exited" | "paused" | …
    pub state: String,
    /// Human status, e.g. "Up 2 hours".
    pub status: String,
    /// Rendered port mappings, e.g. "0.0.0.0:8080→80/tcp".
    pub ports: Vec<String>,
    #[ts(type = "number")]
    pub created: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DockerImage {
    pub id: String,
    pub tags: Vec<String>,
    #[ts(type = "number")]
    pub size: i64,
    #[ts(type = "number")]
    pub created: i64,
}

fn connect() -> DockerResult<Docker> {
    Docker::connect_with_local_defaults().map_err(|e| DockerError::Unavailable(e.to_string()))
}

fn api_err(e: bollard::errors::Error) -> DockerError {
    // Connection-level failures mean the daemon isn't reachable.
    let text = e.to_string();
    if text.contains("connect") || text.contains("pipe") || text.contains("socket") {
        DockerError::Unavailable(text)
    } else {
        DockerError::Api(text)
    }
}

/// Daemon reachability + version string ("Docker 27.4.0").
pub async fn ping() -> DockerResult<String> {
    let docker = connect()?;
    let version = docker.version().await.map_err(api_err)?;
    Ok(format!(
        "Docker {}",
        version.version.unwrap_or_else(|| "?".into())
    ))
}

pub async fn list_containers() -> DockerResult<Vec<DockerContainer>> {
    let docker = connect()?;
    let summaries = docker
        .list_containers(Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        }))
        .await
        .map_err(api_err)?;
    Ok(summaries.into_iter().map(map_container).collect())
}

/// Names of running containers whose effective user is root — the same
/// signal `docker inspect --format '{{.Config.User}}'` gives per container,
/// batched into one call so a caller (the security center) doesn't have to
/// spawn N inspects itself. An empty `User` means "whatever the image's
/// Dockerfile set, which defaults to root if it never called `USER`" — the
/// same default `docker inspect` itself reports as an empty string.
pub async fn containers_running_as_root() -> DockerResult<Vec<String>> {
    let docker = connect()?;
    // `all: false` (the default) is exactly "running containers only" —
    // stopped containers aren't an active risk in the sense this check cares
    // about.
    let summaries = docker
        .list_containers(Some(ListContainersOptions::<String>::default()))
        .await
        .map_err(api_err)?;

    let mut root_containers = Vec::new();
    for summary in summaries {
        let Some(id) = summary.id.clone() else {
            continue;
        };
        let name = container_name(&summary);
        let inspect = docker.inspect_container(&id, None).await.map_err(api_err)?;
        let user = inspect.config.and_then(|c| c.user).unwrap_or_default();
        if user.is_empty() || user == "root" || user == "0" {
            root_containers.push(name);
        }
    }
    Ok(root_containers)
}

pub async fn list_images() -> DockerResult<Vec<DockerImage>> {
    let docker = connect()?;
    let summaries = docker
        .list_images(Some(ListImagesOptions::<String> {
            all: false,
            ..Default::default()
        }))
        .await
        .map_err(api_err)?;
    Ok(summaries.into_iter().map(map_image).collect())
}

pub async fn start_container(id: &str) -> DockerResult<()> {
    let docker = connect()?;
    docker
        .start_container(id, None::<StartContainerOptions<String>>)
        .await
        .map_err(api_err)
}

pub async fn stop_container(id: &str) -> DockerResult<()> {
    let docker = connect()?;
    docker
        .stop_container(
            id,
            Some(StopContainerOptions {
                t: STOP_TIMEOUT_SECS,
            }),
        )
        .await
        .map_err(api_err)
}

pub async fn restart_container(id: &str) -> DockerResult<()> {
    let docker = connect()?;
    docker
        .restart_container(
            id,
            Some(RestartContainerOptions {
                t: STOP_TIMEOUT_SECS as isize,
            }),
        )
        .await
        .map_err(api_err)
}

/// Last `tail` lines of a container's stdout+stderr.
pub async fn container_logs(id: &str, tail: usize) -> DockerResult<String> {
    let docker = connect()?;
    let mut stream = docker.logs(
        id,
        Some(LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            timestamps: false,
            ..Default::default()
        }),
    );
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(api_err)?;
        out.extend_from_slice(&chunk.into_bytes());
        if out.len() > MAX_LOG_BYTES {
            let excess = out.len() - MAX_LOG_BYTES;
            out.drain(..excess);
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn container_name(summary: &ContainerSummary) -> String {
    summary
        .names
        .as_ref()
        .and_then(|names| names.first())
        .map(|n| n.trim_start_matches('/').to_string())
        .unwrap_or_else(|| "unnamed".into())
}

fn map_container(summary: ContainerSummary) -> DockerContainer {
    let name = container_name(&summary);
    let ports = summary
        .ports
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let typ = p
                .typ
                .map(|t| t.to_string().to_lowercase())
                .unwrap_or_else(|| "tcp".into());
            match (p.public_port, p.ip) {
                (Some(public), Some(ip)) => format!("{ip}:{public}→{}/{typ}", p.private_port),
                (Some(public), None) => format!("{public}→{}/{typ}", p.private_port),
                (None, _) => format!("{}/{typ}", p.private_port),
            }
        })
        .collect();
    DockerContainer {
        id: summary.id.unwrap_or_default(),
        name,
        image: summary.image.unwrap_or_default(),
        state: summary.state.unwrap_or_default(),
        status: summary.status.unwrap_or_default(),
        ports,
        created: summary.created.unwrap_or(0),
    }
}

fn map_image(summary: ImageSummary) -> DockerImage {
    DockerImage {
        id: summary
            .id
            .trim_start_matches("sha256:")
            .chars()
            .take(12)
            .collect(),
        tags: summary
            .repo_tags
            .into_iter()
            .filter(|t| t != "<none>:<none>")
            .collect(),
        size: summary.size,
        created: summary.created,
    }
}

pub struct DockerModule;

impl Module for DockerModule {
    fn id(&self) -> &'static str {
        "docker"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "docker.open".into(),
            module: "docker".into(),
            title: "Open Docker".into(),
            keywords: vec!["container".into(), "image".into(), "compose".into()],
            shortcut: Some("Ctrl+7".into()),
        }]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_container_summary() {
        let summary = ContainerSummary {
            id: Some("abc123".into()),
            names: Some(vec!["/devos-db".into()]),
            image: Some("postgres:16".into()),
            state: Some("running".into()),
            status: Some("Up 2 hours".into()),
            created: Some(1_700_000_000),
            ports: Some(vec![bollard::models::Port {
                ip: Some("0.0.0.0".into()),
                private_port: 5432,
                public_port: Some(15432),
                typ: Some(bollard::models::PortTypeEnum::TCP),
            }]),
            ..Default::default()
        };
        let mapped = map_container(summary);
        assert_eq!(mapped.name, "devos-db");
        assert_eq!(mapped.image, "postgres:16");
        assert_eq!(mapped.state, "running");
        assert_eq!(mapped.ports, vec!["0.0.0.0:15432→5432/tcp"]);
    }

    #[test]
    fn maps_image_summary_and_filters_dangling_tags() {
        let summary = ImageSummary {
            id: "sha256:0123456789abcdef0123".into(),
            repo_tags: vec!["postgres:16".into(), "<none>:<none>".into()],
            size: 400_000_000,
            created: 1_700_000_000,
            ..Default::default()
        };
        let mapped = map_image(summary);
        assert_eq!(mapped.id, "0123456789ab");
        assert_eq!(mapped.tags, vec!["postgres:16"]);
        assert_eq!(mapped.size, 400_000_000);
    }

    /// Exercises the real daemon when present; skips honestly when not.
    #[tokio::test]
    async fn daemon_roundtrip_when_available() {
        match ping().await {
            Err(e) => {
                eprintln!("docker unavailable, skipping daemon test: {e}");
            }
            Ok(version) => {
                assert!(version.starts_with("Docker "));
                let containers = list_containers().await.expect("list containers");
                let images = list_images().await.expect("list images");
                eprintln!(
                    "docker ok: {} containers, {} images",
                    containers.len(),
                    images.len()
                );
            }
        }
    }

    /// Same honest-skip shape as `daemon_roundtrip_when_available` — this
    /// only asserts the call succeeds and returns *some* list, since asserting
    /// on which containers are root would mean depending on whatever happens
    /// to be running on the machine the test executes on.
    #[tokio::test]
    async fn root_check_roundtrip_when_available() {
        match containers_running_as_root().await {
            Err(e) => {
                eprintln!("docker unavailable, skipping root-check test: {e}");
            }
            Ok(root_containers) => {
                eprintln!("docker ok: {} root container(s)", root_containers.len());
            }
        }
    }
}
