//! Dependency vulnerability audits: `cargo audit` for a Rust project,
//! `npm`/`pnpm audit` for a JS one. Both are optional external tools this
//! app cannot assume are installed, so "not installed" is reported as its
//! own status rather than folded into a generic error — the whole point is
//! telling the user what to do next, and "run `cargo install cargo-audit`"
//! is a different next step than "something went wrong."

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

const AUDIT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum DependencyStatus {
    Ok,
    Vulnerable,
    ToolNotInstalled,
    /// A real ecosystem this crate deliberately does not parse yet (`yarn
    /// audit` emits NDJSON, a different shape from npm/pnpm's single JSON
    /// object) — distinct from `Error` because nothing went wrong, this
    /// path just isn't built.
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DependencyCheck {
    pub ecosystem: String,
    pub status: DependencyStatus,
    #[ts(type = "number | null")]
    pub vulnerability_count: Option<i64>,
    pub detail: Option<String>,
}

/// Every dependency audit relevant to `root`: `cargo audit` if it has a
/// `Cargo.toml`, `npm`/`pnpm audit` if it has a `package.json`. A project
/// with neither runs no audit and reports nothing — not an error, there is
/// simply nothing to check.
pub async fn audit(root: &Path) -> Vec<DependencyCheck> {
    let mut checks = Vec::new();
    if root.join("Cargo.toml").is_file() {
        checks.push(cargo_audit(root).await);
    }
    if root.join("package.json").is_file() {
        checks.push(js_audit(root).await);
    }
    checks
}

async fn cargo_audit(root: &Path) -> DependencyCheck {
    let output = run("cargo audit --json", root).await;
    match output {
        Ok(RunOutput { stdout, stderr, .. }) => {
            if let Some(count) = extract_count(&stdout, &["vulnerabilities", "count"]) {
                return ok_or_vulnerable("cargo", count);
            }
            if not_installed(&stderr) {
                return DependencyCheck {
                    ecosystem: "cargo".into(),
                    status: DependencyStatus::ToolNotInstalled,
                    vulnerability_count: None,
                    detail: Some(
                        "cargo-audit is not installed — run `cargo install cargo-audit`".into(),
                    ),
                };
            }
            error("cargo", first_line(&stderr).or_else(|| first_line(&stdout)))
        }
        Err(detail) => error("cargo", Some(detail)),
    }
}

async fn js_audit(root: &Path) -> DependencyCheck {
    // Same detection order `test_runner::detect_js_script` uses: a lockfile
    // names the manager, because that is what the project actually runs
    // with, not a guess.
    if root.join("yarn.lock").is_file() {
        return DependencyCheck {
            ecosystem: "yarn".into(),
            status: DependencyStatus::Unsupported,
            vulnerability_count: None,
            detail: Some("yarn audit isn't parsed yet — run `yarn audit` directly".into()),
        };
    }
    let manager = if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else {
        "npm"
    };

    let output = run(&format!("{manager} audit --json"), root).await;
    match output {
        Ok(RunOutput { stdout, stderr, .. }) => {
            if let Some(count) = extract_count(&stdout, &["metadata", "vulnerabilities", "total"]) {
                return ok_or_vulnerable(manager, count);
            }
            if not_installed(&stderr) {
                return DependencyCheck {
                    ecosystem: manager.into(),
                    status: DependencyStatus::ToolNotInstalled,
                    vulnerability_count: None,
                    detail: Some(format!("{manager} was not found on PATH")),
                };
            }
            error(manager, first_line(&stderr).or_else(|| first_line(&stdout)))
        }
        Err(detail) => error(manager, Some(detail)),
    }
}

fn ok_or_vulnerable(ecosystem: &str, count: i64) -> DependencyCheck {
    DependencyCheck {
        ecosystem: ecosystem.into(),
        status: if count > 0 {
            DependencyStatus::Vulnerable
        } else {
            DependencyStatus::Ok
        },
        vulnerability_count: Some(count),
        detail: None,
    }
}

fn error(ecosystem: &str, detail: Option<String>) -> DependencyCheck {
    DependencyCheck {
        ecosystem: ecosystem.into(),
        status: DependencyStatus::Error,
        vulnerability_count: None,
        detail,
    }
}

/// Walks a fixed path of object keys (`vulnerabilities.count`,
/// `metadata.vulnerabilities.total`) out of the tool's JSON, if `stdout`
/// parses as JSON at all — the audit ran and answered, whatever its exit
/// code said. Vulnerability-finding audit tools routinely exit non-zero to
/// mean "found some," not "failed."
fn extract_count(stdout: &str, path: &[&str]) -> Option<i64> {
    let json: Value = serde_json::from_str(stdout).ok()?;
    let mut cursor = &json;
    for key in path {
        cursor = cursor.get(key)?;
    }
    cursor.as_i64()
}

fn not_installed(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("no such command") || lower.contains("is not recognized")
}

fn first_line(text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    Some(line.trim().to_string())
}

struct RunOutput {
    stdout: String,
    stderr: String,
}

async fn run(command: &str, cwd: &Path) -> Result<RunOutput, String> {
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    cmd.current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let output = tokio::time::timeout(AUDIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("timed out after {}s", AUDIT_TIMEOUT.as_secs()))?
        .map_err(|e| e.to_string())?;

    Ok(RunOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_nested_count_when_present() {
        let stdout = r#"{"vulnerabilities":{"count":3,"list":[]}}"#;
        assert_eq!(
            extract_count(stdout, &["vulnerabilities", "count"]),
            Some(3)
        );
    }

    #[test]
    fn returns_none_for_output_that_is_not_json() {
        assert_eq!(
            extract_count("not json", &["vulnerabilities", "count"]),
            None
        );
    }

    #[test]
    fn returns_none_when_the_path_is_not_present() {
        let stdout = r#"{"ok":true}"#;
        assert_eq!(extract_count(stdout, &["vulnerabilities", "count"]), None);
    }

    #[test]
    fn recognizes_both_platforms_no_such_command_phrasing() {
        assert!(not_installed("error: no such command: `audit`"));
        assert!(not_installed(
            "'cargo-audit' is not recognized as an internal or external command"
        ));
        assert!(!not_installed("2 vulnerabilities found"));
    }

    #[tokio::test]
    async fn a_project_with_neither_manifest_gets_no_checks() {
        let dir = tempfile::tempdir().unwrap();
        assert!(audit(dir.path()).await.is_empty());
    }

    #[tokio::test]
    async fn a_yarn_project_is_reported_unsupported_not_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();

        let checks = audit(dir.path()).await;
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].ecosystem, "yarn");
        assert_eq!(checks[0].status, DependencyStatus::Unsupported);
    }
}
