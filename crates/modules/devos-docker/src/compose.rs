//! Docker Compose, via the real `docker compose` CLI rather than hand-rolled
//! through bollard. Bollard is a pure Engine API client — Compose is a
//! separate spec/CLI layer (dependency graphs, `depends_on` ordering,
//! variable interpolation, override-file merging) that only the real tool
//! implements correctly. Same reasoning `devos-git/src/cli.rs` gives for
//! shelling out to `git` rather than reimplementing it against libgit2,
//! applying more strongly here: reimplementing Compose's orchestration in
//! Rust would inevitably drift from what `docker compose` actually does.
//!
//! Status for each declared service still comes from bollard
//! (`devos_docker::list_containers`, filtered by the `com.docker.compose.service`
//! label Compose attaches automatically) rather than parsing `compose ps`'s
//! own JSON — one source of container truth, not two that can disagree.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use ts_rs::TS;

/// Local operations only (`up`/`down`/`ps`/`restart`) — nothing here pulls
/// images or otherwise reaches the network, so one budget covers all of it,
/// unlike `devos-git`'s local-vs-network split.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Priority order matches Compose's own file-discovery precedence.
const COMPOSE_FILENAMES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    #[error("docker compose is not installed")]
    NotInstalled,
    #[error("docker compose {command} failed: {stderr}")]
    Failed { command: String, stderr: String },
    #[error("docker compose {0} timed out")]
    Timeout(String),
    #[error("could not run docker compose: {0}")]
    Io(#[from] std::io::Error),
}

pub type ComposeResult<T> = Result<T, ComposeError>;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ComposeProject {
    pub file: String,
    /// The `-p`/`--project-name` Compose derives when nothing overrides it:
    /// the compose file's own directory name, lowercased and sanitized —
    /// the same value the `com.docker.compose.project` label carries, which
    /// is how a running container is matched back to this file.
    pub name: String,
    pub services: Vec<String>,
}

/// The first compose file found in `dir`, by Compose's own precedence.
/// `None` means there is nothing to show a Compose section for at all —
/// distinct from a project that has one but nothing running yet.
pub fn find_compose_file(dir: &Path) -> Option<PathBuf> {
    COMPOSE_FILENAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

/// Compose's own default project-name derivation: the directory the compose
/// file lives in, lowercased, with everything but `[a-z0-9_-]` dropped —
/// mirrored here (rather than shelled out for) because it's needed to
/// cross-reference bollard's container list, and asking `docker compose`
/// for a name it derives from the same rule would just be a slower way to
/// compute it.
fn default_project_name(compose_file: &Path) -> String {
    let dir_name = compose_file
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir_name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

/// The services a compose file declares, via `config --services` — every
/// name it lists, regardless of whether anything has been started for it
/// yet, which `ps` alone would miss for a service never brought up.
pub async fn services(compose_file: &Path) -> ComposeResult<ComposeProject> {
    let output = run(compose_file, &["config", "--services"]).await?;
    let services = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    Ok(ComposeProject {
        file: compose_file.display().to_string(),
        name: default_project_name(compose_file),
        services,
    })
}

pub async fn up(compose_file: &Path, service: Option<&str>) -> ComposeResult<()> {
    let mut args = vec!["up", "-d"];
    if let Some(service) = service {
        args.push(service);
    }
    run(compose_file, &args).await?;
    Ok(())
}

pub async fn down(compose_file: &Path) -> ComposeResult<()> {
    run(compose_file, &["down"]).await?;
    Ok(())
}

pub async fn restart(compose_file: &Path, service: Option<&str>) -> ComposeResult<()> {
    let mut args = vec!["restart"];
    if let Some(service) = service {
        args.push(service);
    }
    run(compose_file, &args).await?;
    Ok(())
}

/// Runs `docker compose -f <file> <args>` from the file's own directory —
/// relative paths a compose file references (build contexts, env files,
/// bind mounts) resolve the same way they would from a terminal sitting in
/// that directory.
async fn run(compose_file: &Path, args: &[&str]) -> ComposeResult<String> {
    let dir = compose_file.parent().unwrap_or_else(|| Path::new("."));
    let file_arg = compose_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| compose_file.display().to_string());

    let mut cmd = Command::new("docker");
    cmd.args(["compose", "-f", &file_arg])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let command_name = format!("compose {}", args.join(" "));
    let output = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| ComposeError::Timeout(command_name.clone()))??;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("not recognized")
            || stderr.contains("no such command")
            || stderr.contains("is not a docker command")
        {
            Err(ComposeError::NotInstalled)
        } else {
            Err(ComposeError::Failed {
                command: command_name,
                stderr,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_compose_yaml_before_docker_compose_yml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docker-compose.yml"), "").unwrap();
        std::fs::write(dir.path().join("compose.yaml"), "").unwrap();
        let found = find_compose_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "compose.yaml");
    }

    #[test]
    fn reports_none_when_no_compose_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_compose_file(dir.path()).is_none());
    }

    #[test]
    fn default_project_name_sanitizes_the_directory_name() {
        let path = Path::new("/home/user/My Cool App!/compose.yaml");
        assert_eq!(default_project_name(path), "mycoolapp");
    }
}
