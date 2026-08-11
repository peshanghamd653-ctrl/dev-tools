//! Thin wrapper around the user's `git` executable. Using the real CLI keeps
//! credentials, hooks, LFS, and config behavior identical to their terminal.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("not a git repository: {0}")]
    NotARepo(String),
    #[error("git {command} failed: {stderr}")]
    Failed { command: String, stderr: String },
    #[error("git {0} timed out")]
    Timeout(String),
    #[error("could not run git: {0}")]
    Io(#[from] std::io::Error),
}

pub type GitResult<T> = Result<T, GitError>;

/// Run git in `repo` and return stdout. Local operations get a 30s guard;
/// network operations (push/pull/fetch/clone) get 300s.
pub async fn run_git(repo: &Path, args: &[&str]) -> GitResult<String> {
    run_git_with_env(repo, args, &[]).await
}

/// Same as [`run_git`], with extra environment variables set on the child
/// process. Exists for `rebase --continue`/`--skip`, which must never stop
/// to open an interactive editor — `GIT_EDITOR`/`GIT_SEQUENCE_EDITOR=true`
/// makes any editor git would otherwise launch a no-op instead.
pub async fn run_git_with_env(
    repo: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> GitResult<String> {
    let network_op = matches!(
        args.first().copied(),
        Some("push") | Some("pull") | Some("fetch") | Some("clone")
    );
    let timeout = if network_op {
        Duration::from_secs(300)
    } else {
        Duration::from_secs(30)
    };

    let mut cmd = Command::new("git");
    cmd.arg("--no-optional-locks")
        .args(args)
        .current_dir(repo)
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Don't flash console windows on Windows.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let command_name = args.first().copied().unwrap_or("git").to_string();
    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| GitError::Timeout(command_name.clone()))??;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("not a git repository") {
            Err(GitError::NotARepo(repo.display().to_string()))
        } else {
            Err(GitError::Failed {
                command: command_name,
                stderr,
            })
        }
    }
}
