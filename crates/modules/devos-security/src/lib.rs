//! Security center: a point-in-time snapshot of three things a developer
//! wants to know are true before they trust a project — the working tree is
//! clean, nothing that looks like a credential is sitting in a tracked
//! file, and the dependency tree has no known vulnerabilities.
//!
//! Deliberately not attempted in this pass: outdated-package detection (no
//! standard `cargo` subcommand for it, and `npm outdated`'s exit-code and
//! output shape are different enough from `npm audit`'s to be a second
//! feature) and a Docker "running as root" check (needs `devos-docker`'s
//! container inspection extended with something it doesn't expose yet).
//! Both are real gaps, named rather than silently absent.
//!
//! Nothing here mutates anything — every check is a read (a `git status`,
//! a file walk, spawning an audit tool that itself only reads a lockfile)
//! — so there is no approval gate to design, unlike the AI tool-calling
//! surface this crate's secret-detection dependency (`devos-redact`) also
//! serves.

mod audit;
mod secrets;

pub use audit::{audit, DependencyCheck, DependencyStatus};
pub use secrets::{scan as scan_secrets, SecretFinding, SecretScan};

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GitCheck {
    pub is_repo: bool,
    pub clean: bool,
    #[ts(type = "number")]
    pub changed_files: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SecurityReport {
    pub git: GitCheck,
    pub secrets: SecretScan,
    pub dependencies: Vec<DependencyCheck>,
}

async fn git_check(root: &Path) -> GitCheck {
    match devos_git::status(root).await {
        Ok((info, entries)) if info.is_repo => GitCheck {
            is_repo: true,
            clean: entries.is_empty(),
            changed_files: entries.len() as i64,
        },
        _ => GitCheck {
            is_repo: false,
            clean: true,
            changed_files: 0,
        },
    }
}

/// The whole report for one project root, in one call — the UI's "Scan"
/// button asks for all three checks together rather than issuing three
/// round trips it would have to coordinate the loading state of.
pub async fn scan(root: &Path) -> SecurityReport {
    SecurityReport {
        git: git_check(root).await,
        secrets: scan_secrets(root),
        dependencies: audit(root).await,
    }
}

pub struct SecurityModule;

impl Module for SecurityModule {
    fn id(&self) -> &'static str {
        "security"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "security.open".into(),
            module: "security".into(),
            title: "Security Center".into(),
            keywords: vec![
                "security".into(),
                "audit".into(),
                "secrets".into(),
                "vulnerabilities".into(),
            ],
            shortcut: None,
        }]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_non_git_directory_reports_not_a_repo_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let check = git_check(dir.path()).await;
        assert!(!check.is_repo);
        assert!(check.clean, "nothing to be dirty about outside a repo");
    }

    #[tokio::test]
    async fn scan_combines_all_three_checks_for_a_plain_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "just notes\n").unwrap();

        let report = scan(dir.path()).await;
        assert!(!report.git.is_repo);
        assert_eq!(report.secrets.files_scanned, 1);
        assert!(report.dependencies.is_empty(), "no manifest, no audit");
    }
}
