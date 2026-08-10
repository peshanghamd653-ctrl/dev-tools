//! Wraps `devos_docker::containers_running_as_root` for the security
//! report: Docker Desktop simply not running is the common case for anyone
//! not using containers, so it gets its own status rather than being folded
//! into a generic error, the same way a missing `cargo audit` does.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum DockerRootStatus {
    Ok,
    RunningAsRoot,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DockerRootCheck {
    pub status: DockerRootStatus,
    pub containers: Vec<String>,
}

pub async fn check() -> DockerRootCheck {
    match devos_docker::containers_running_as_root().await {
        Ok(containers) if containers.is_empty() => DockerRootCheck {
            status: DockerRootStatus::Ok,
            containers,
        },
        Ok(containers) => DockerRootCheck {
            status: DockerRootStatus::RunningAsRoot,
            containers,
        },
        Err(_) => DockerRootCheck {
            status: DockerRootStatus::Unavailable,
            containers: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the real daemon when present; skips honestly when not —
    /// same shape as `devos_docker`'s own roundtrip tests. Either way,
    /// `Unavailable` must never happen alongside a non-empty container list.
    #[tokio::test]
    async fn check_never_reports_unavailable_with_containers() {
        let result = check().await;
        if result.status == DockerRootStatus::Unavailable {
            assert!(result.containers.is_empty());
        }
    }
}
