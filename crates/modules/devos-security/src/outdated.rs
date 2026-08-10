//! Outdated-package detection: `npm`/`pnpm outdated --json`/`--format json`
//! for a JS project. Cargo deliberately gets no equivalent — verified by
//! actually installing `cargo-outdated` and running `cargo outdated
//! --format json` against this workspace rather than assuming: its output
//! is NDJSON, one JSON object per workspace crate covering that crate's
//! *entire* transitive dependency tree (hundreds of entries, most of them
//! nothing a user could act on), and resolving that against the registry
//! took several minutes on a workspace of ~20 crates. Neither the shape nor
//! the latency is something to run on every "Scan" click, so cargo is
//! reported `Unsupported`, the same status (and the same reasoning) as
//! yarn's NDJSON `outdated`/`audit` output.
//!
//! A separate check from `audit::audit` rather than a mode on it: audit
//! JSON nests a count under a fixed key path, outdated JSON for npm/pnpm
//! *is* the map of outdated packages itself, one entry per package name —
//! different enough shapes that sharing one parser would just be two
//! `Option`-returning branches pretending to be one.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::audit::{first_line, not_installed, run, RunOutput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum OutdatedStatus {
    Ok,
    Outdated,
    ToolNotInstalled,
    /// cargo (NDJSON, whole-tree, slow — see the module doc comment) and
    /// yarn (NDJSON, different shape from npm/pnpm) both land here.
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct OutdatedCheck {
    pub ecosystem: String,
    pub status: OutdatedStatus,
    #[ts(type = "number | null")]
    pub outdated_count: Option<i64>,
    pub detail: Option<String>,
}

/// Every outdated-package check relevant to `root`, same manifest-detection
/// rule `audit::audit` uses.
pub async fn outdated(root: &Path) -> Vec<OutdatedCheck> {
    let mut checks = Vec::new();
    if root.join("Cargo.toml").is_file() {
        checks.push(cargo_outdated());
    }
    if root.join("package.json").is_file() {
        checks.push(js_outdated(root).await);
    }
    checks
}

/// Never shells out — see the module doc comment for why running
/// `cargo outdated` isn't worth doing on every scan even when it's
/// installed.
fn cargo_outdated() -> OutdatedCheck {
    OutdatedCheck {
        ecosystem: "cargo".into(),
        status: OutdatedStatus::Unsupported,
        outdated_count: None,
        detail: Some(
            "cargo-outdated's JSON output isn't parsed here (NDJSON, whole \
             dependency tree, multi-minute runtime) — run `cargo outdated` \
             directly"
                .into(),
        ),
    }
}

async fn js_outdated(root: &Path) -> OutdatedCheck {
    if root.join("yarn.lock").is_file() {
        return OutdatedCheck {
            ecosystem: "yarn".into(),
            status: OutdatedStatus::Unsupported,
            outdated_count: None,
            detail: Some("yarn outdated isn't parsed yet — run `yarn outdated` directly".into()),
        };
    }
    let (manager, flag) = if root.join("pnpm-lock.yaml").is_file() {
        ("pnpm", "--format json")
    } else {
        ("npm", "--json")
    };

    let output = run(&format!("{manager} outdated {flag}"), root).await;
    match output {
        Ok(RunOutput { stdout, stderr, .. }) => {
            if let Some(count) = count_package_map(&stdout) {
                return ok_or_outdated(manager, count);
            }
            if not_installed(&stderr) {
                return OutdatedCheck {
                    ecosystem: manager.into(),
                    status: OutdatedStatus::ToolNotInstalled,
                    outdated_count: None,
                    detail: Some(format!("{manager} was not found on PATH")),
                };
            }
            error(manager, first_line(&stderr).or_else(|| first_line(&stdout)))
        }
        Err(detail) => error(manager, Some(detail)),
    }
}

fn ok_or_outdated(ecosystem: &str, count: i64) -> OutdatedCheck {
    OutdatedCheck {
        ecosystem: ecosystem.into(),
        status: if count > 0 {
            OutdatedStatus::Outdated
        } else {
            OutdatedStatus::Ok
        },
        outdated_count: Some(count),
        detail: None,
    }
}

fn error(ecosystem: &str, detail: Option<String>) -> OutdatedCheck {
    OutdatedCheck {
        ecosystem: ecosystem.into(),
        status: OutdatedStatus::Error,
        outdated_count: None,
        detail,
    }
}

/// `npm`/`pnpm outdated --json`'s whole output *is* the map of outdated
/// packages — one key per package, `{}` when there are none. Verified
/// directly against both tools (not just documentation): `npm outdated
/// --json` exits 1 with outdated packages, `pnpm outdated --format json`
/// exits 0 either way — `run` doesn't care about exit status, only that the
/// output parses.
fn count_package_map(stdout: &str) -> Option<i64> {
    let json: Value = serde_json::from_str(stdout).ok()?;
    Some(json.as_object()?.len() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_npm_style_package_map() {
        let stdout = r#"{"eslint":{"current":"9.0.0","latest":"9.1.0"},"vite":{"current":"7.0.0","latest":"7.1.0"}}"#;
        assert_eq!(count_package_map(stdout), Some(2));
    }

    #[test]
    fn an_empty_package_map_is_zero_not_none() {
        assert_eq!(count_package_map("{}"), Some(0));
    }

    #[test]
    fn returns_none_for_output_that_is_not_json() {
        assert_eq!(count_package_map("not json"), None);
    }

    #[tokio::test]
    async fn a_project_with_neither_manifest_gets_no_checks() {
        let dir = tempfile::tempdir().unwrap();
        assert!(outdated(dir.path()).await.is_empty());
    }

    #[tokio::test]
    async fn a_cargo_project_is_reported_unsupported_without_running_anything() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();

        let checks = outdated(dir.path()).await;
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].ecosystem, "cargo");
        assert_eq!(checks[0].status, OutdatedStatus::Unsupported);
    }

    #[tokio::test]
    async fn a_yarn_project_is_reported_unsupported_not_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();

        let checks = outdated(dir.path()).await;
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].ecosystem, "yarn");
        assert_eq!(checks[0].status, OutdatedStatus::Unsupported);
    }

    /// Exercises real `npm outdated --json` on a project with no
    /// dependencies at all — the honest way to check `Ok` without asserting
    /// on the exact package count of whatever this crate's real
    /// dependencies happen to be.
    #[tokio::test]
    async fn a_project_with_no_dependencies_reports_ok() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"empty","version":"1.0.0"}"#,
        )
        .unwrap();

        let checks = outdated(dir.path()).await;
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].ecosystem, "npm");
        assert_eq!(checks[0].status, OutdatedStatus::Ok);
        assert_eq!(checks[0].outdated_count, Some(0));
    }
}
