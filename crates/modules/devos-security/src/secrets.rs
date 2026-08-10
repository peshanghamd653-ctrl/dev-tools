//! Project-wide secret scan: walk the tree, run every text file through
//! `devos_redact`, and report what shape of credential turned up where —
//! never the credential itself. Same skip-list and size ceiling
//! `devos-index` uses for its own walk, so a scan and an index pass agree on
//! what counts as "the project" without either crate depending on the other.

use std::path::Path;

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
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_WALK_DEPTH: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SecretFinding {
    pub file: String,
    pub kind: String,
    #[ts(type = "number")]
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SecretScan {
    #[ts(type = "number")]
    pub files_scanned: i64,
    pub findings: Vec<SecretFinding>,
}

pub fn scan(root: &Path) -> SecretScan {
    let mut files_scanned = 0i64;
    let mut findings = Vec::new();
    walk(root, root, 0, &mut files_scanned, &mut findings);
    SecretScan {
        files_scanned,
        findings,
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    files_scanned: &mut i64,
    findings: &mut Vec<SecretFinding>,
) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(root, &path, depth + 1, files_scanned, findings);
            }
            continue;
        }
        if meta.len() == 0 || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        scan_file(root, &path, files_scanned, findings);
    }
}

fn scan_file(root: &Path, path: &Path, files_scanned: &mut i64, findings: &mut Vec<SecretFinding>) {
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    // A binary file's random bytes will never match a credential pattern
    // meaningfully, and decoding it lossy just risks a slow, noisy scan.
    if bytes.contains(&0) {
        return;
    }
    let text = String::from_utf8_lossy(&bytes);
    *files_scanned += 1;

    let relative = relative_display(root, path);
    for finding in devos_redact::redact(&text).findings {
        findings.push(SecretFinding {
            file: relative.clone(),
            kind: finding.kind.to_string(),
            line: finding.line,
        });
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

    #[test]
    fn a_clean_project_reports_no_findings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        let result = scan(dir.path());
        assert_eq!(result.files_scanned, 1);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn a_secret_in_a_tracked_file_is_found_with_its_location() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/.env"),
            "PORT=3000\nOPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456\n",
        )
        .unwrap();
        let result = scan(dir.path());
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].file, "src/.env");
        assert_eq!(result.findings[0].line, 2);
    }

    #[test]
    fn dependency_and_vcs_directories_are_never_walked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        std::fs::write(
            dir.path().join("node_modules/pkg/index.js"),
            "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456",
        )
        .unwrap();
        let result = scan(dir.path());
        assert_eq!(result.files_scanned, 0);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn a_binary_file_is_skipped_rather_than_scanned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("logo.png"), [0u8, 1, 2, 3, 0, 255]).unwrap();
        let result = scan(dir.path());
        assert_eq!(result.files_scanned, 0);
    }

    #[test]
    fn findings_never_carry_the_matched_text_itself() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "DATABASE_PASSWORD=correcthorsebatterystaple",
        )
        .unwrap();
        let result = scan(dir.path());
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].kind, "env-style-secret");
        // Nothing on the Finding carries the value — this would not even
        // compile if it did, but the point is worth asserting explicitly.
        assert!(!format!("{:?}", result.findings[0]).contains("correcthorsebatterystaple"));
    }
}
