//! Finds `.env`-style files that would be committed as-is if `git add` were
//! run on them — a leak that needs no credential *pattern* to match, since
//! the whole file is the risk, template files included. Only meaningful
//! inside a git repo; outside one there is no "ignored or not" to report,
//! so the check comes back empty rather than guessing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "__pycache__",
];
const MAX_WALK_DEPTH: usize = 12;
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// These are meant to be committed — they document the shape of the real
/// file without holding real values.
const TEMPLATE_SUFFIXES: &[&str] = &["example", "sample", "template", "defaults", "dist"];

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct EnvFileCheck {
    pub files: Vec<String>,
}

pub async fn check(root: &Path) -> EnvFileCheck {
    if !root.join(".git").exists() {
        return EnvFileCheck { files: Vec::new() };
    }
    let mut candidates = Vec::new();
    walk(root, 0, &mut candidates);

    let mut files = Vec::new();
    for candidate in candidates {
        if !is_ignored(root, &candidate).await {
            files.push(relative_display(root, &candidate));
        }
    }
    EnvFileCheck { files }
}

fn walk(dir: &Path, depth: usize, candidates: &mut Vec<PathBuf>) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(&path, depth + 1, candidates);
            }
            continue;
        }
        if is_env_file(&name) {
            candidates.push(path);
        }
    }
}

fn is_env_file(name: &str) -> bool {
    if name == ".env" {
        return true;
    }
    match name.strip_prefix(".env.") {
        Some(suffix) => !TEMPLATE_SUFFIXES.contains(&suffix),
        None => false,
    }
}

/// `git check-ignore` exits 0 when the path is ignored and 1 when it is
/// not — the one case this function reports `false` for. Anything else
/// (git missing, a fatal error) is "can't tell," and is reported as
/// ignored rather than risk a false alarm from a tool that isn't actually
/// working, matching how a missing `cargo audit`/`npm audit` is its own
/// status rather than a reported vulnerability.
async fn is_ignored(root: &Path, path: &Path) -> bool {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["check-ignore", "--quiet"])
        .arg(path)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    match tokio::time::timeout(CHECK_TIMEOUT, cmd.status()).await {
        Ok(Ok(status)) => status.code() != Some(1),
        _ => true,
    }
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_git_repo(dir: &Path) {
        let status = tokio::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .status()
            .await
            .expect("git init");
        assert!(status.success());
    }

    #[tokio::test]
    async fn outside_a_git_repo_nothing_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();
        let result = check(dir.path()).await;
        assert!(result.files.is_empty());
    }

    #[tokio::test]
    async fn an_env_file_with_no_gitignore_entry_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();

        let result = check(dir.path()).await;
        assert_eq!(result.files, vec![".env".to_string()]);
    }

    #[tokio::test]
    async fn a_gitignored_env_file_is_not_reported() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        std::fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();

        let result = check(dir.path()).await;
        assert!(result.files.is_empty());
    }

    #[tokio::test]
    async fn template_env_files_are_never_flagged() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        std::fs::write(dir.path().join(".env.example"), "SECRET=changeme").unwrap();

        let result = check(dir.path()).await;
        assert!(result.files.is_empty());
    }

    #[tokio::test]
    async fn a_nested_env_file_is_found_and_reported_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(dir.path().join("api/.env.local"), "SECRET=1").unwrap();

        let result = check(dir.path()).await;
        assert_eq!(result.files, vec!["api/.env.local".to_string()]);
    }
}
