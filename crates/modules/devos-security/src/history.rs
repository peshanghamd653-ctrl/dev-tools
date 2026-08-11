//! Git *history* secret scan — the gap `secrets::scan` names in its own doc
//! comment: that walk only ever sees files on disk today, so a credential
//! committed and later removed from the working tree is never caught.
//!
//! Only *added* lines (`+` in `git log -p`) are scanned, not removed or
//! context lines. Context lines would mean the same long-lived secret gets
//! reported once per commit that merely touches nearby code — noise, not
//! signal — and removed lines aren't a leak event. A line entering history
//! is: exactly the moment covered.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Commits scanned, most recent first. Deep enough to catch a realistic
/// "committed then reverted a few commits later" mistake without the cost
/// of a large, old repo's entire history — this is a bounded best-effort
/// check, not an exhaustive audit.
const MAX_COMMITS: usize = 200;
/// A delimiter `git log`'s own format text will not produce by accident,
/// making commit boundaries a plain string split rather than a regex.
const COMMIT_MARKER: &str = "\u{1}DEVOS-COMMIT\u{1}";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct HistorySecretFinding {
    /// Short (7-character) commit hash — enough to `git show`/`git log` it,
    /// short enough to display in a list.
    pub commit: String,
    pub file: String,
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum HistorySecretScanStatus {
    Ok,
    NotARepo,
    /// A repository with no commits yet — nothing to walk, not an error.
    NoHistory,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct HistorySecretScan {
    pub status: HistorySecretScanStatus,
    #[ts(type = "number")]
    pub commits_scanned: i64,
    pub findings: Vec<HistorySecretFinding>,
    pub detail: Option<String>,
}

fn ok(commits_scanned: i64, findings: Vec<HistorySecretFinding>) -> HistorySecretScan {
    HistorySecretScan {
        status: HistorySecretScanStatus::Ok,
        commits_scanned,
        findings,
        detail: None,
    }
}

pub async fn scan(root: &Path) -> HistorySecretScan {
    let output = devos_git::run_git(
        root,
        &[
            "log",
            &format!("--format={COMMIT_MARKER}%H"),
            "-p",
            "-n",
            &MAX_COMMITS.to_string(),
        ],
    )
    .await;

    let log = match output {
        Ok(log) => log,
        Err(devos_git::GitError::NotARepo(_)) => {
            return HistorySecretScan {
                status: HistorySecretScanStatus::NotARepo,
                commits_scanned: 0,
                findings: Vec::new(),
                detail: None,
            }
        }
        // "does not have any commits yet" is `git log`'s failure mode on a
        // freshly-initialized repo — nothing to scan, not a real error.
        Err(devos_git::GitError::Failed { stderr, .. })
            if stderr.contains("does not have any commits yet") =>
        {
            return HistorySecretScan {
                status: HistorySecretScanStatus::NoHistory,
                commits_scanned: 0,
                findings: Vec::new(),
                detail: None,
            }
        }
        Err(e) => {
            return HistorySecretScan {
                status: HistorySecretScanStatus::Error,
                commits_scanned: 0,
                findings: Vec::new(),
                detail: Some(e.to_string()),
            }
        }
    };

    let mut commits_scanned = 0i64;
    let mut findings = Vec::new();
    for commit_block in log.split(COMMIT_MARKER).filter(|b| !b.trim().is_empty()) {
        let Some((hash, diff)) = commit_block.split_once('\n') else {
            continue;
        };
        commits_scanned += 1;
        let short_hash = &hash[..hash.len().min(7)];
        findings.extend(findings_in_commit(short_hash, diff));
    }

    ok(commits_scanned, findings)
}

/// One commit's `git log -p` diff body, already split from its hash line.
fn findings_in_commit(short_hash: &str, diff: &str) -> Vec<HistorySecretFinding> {
    let mut findings = Vec::new();
    for file_diff in split_file_diffs(diff) {
        let added = added_lines(file_diff.body);
        if added.trim().is_empty() {
            continue;
        }
        for finding in devos_redact::redact(&added).findings {
            findings.push(HistorySecretFinding {
                commit: short_hash.to_string(),
                file: file_diff.path.clone(),
                kind: finding.kind.to_string(),
            });
        }
    }
    findings
}

struct FileDiff<'a> {
    path: String,
    body: &'a str,
}

/// Splits a commit's diff on `diff --git a/... b/...` headers, pairing each
/// span with the path git reports it as *becoming* (`b/`) — the name a
/// rename or a fresh add ends at, which is what matters for "where would a
/// user look for this now."
fn split_file_diffs(diff: &str) -> Vec<FileDiff<'_>> {
    let mut out = Vec::new();
    let mut rest = diff;
    while let Some(header_start) = rest.find("diff --git a/") {
        let after_this_header = &rest[header_start + "diff --git a/".len()..];
        let Some(line_end) = after_this_header.find('\n') else {
            break;
        };
        let header_line = &after_this_header[..line_end];
        let Some(b_pos) = header_line.rfind(" b/") else {
            rest = &after_this_header[line_end + 1..];
            continue;
        };
        let path = header_line[b_pos + " b/".len()..].to_string();

        let body_start = header_start + "diff --git a/".len() + line_end + 1;
        let next_header_offset = rest[body_start..].find("diff --git a/");
        let body_end = next_header_offset.map_or(rest.len(), |o| body_start + o);
        out.push(FileDiff {
            path,
            body: &rest[body_start..body_end],
        });
        rest = &rest[body_end..];
    }
    out
}

/// Every `+` line's content, `+++` file header excluded, newline-joined so
/// line-spanning patterns (like a PEM block) can still match across what
/// were separate added lines in the same hunk.
fn added_lines(body: &str) -> String {
    body.lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .map(|line| &line[1..])
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_git_repo(dir: &Path) {
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            let status = tokio::process::Command::new("git")
                .args(&args)
                .current_dir(dir)
                .status()
                .await
                .expect("git setup");
            assert!(status.success(), "git {args:?} failed");
        }
    }

    async fn commit_file(dir: &Path, name: &str, content: &str, message: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        for args in [vec!["add", "."], vec!["commit", "--quiet", "-m", message]] {
            let status = tokio::process::Command::new("git")
                .args(&args)
                .current_dir(dir)
                .status()
                .await
                .expect("git commit");
            assert!(status.success(), "git {args:?} failed");
        }
    }

    #[tokio::test]
    async fn outside_a_git_repo_reports_not_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        let result = scan(dir.path()).await;
        assert_eq!(result.status, HistorySecretScanStatus::NotARepo);
    }

    #[tokio::test]
    async fn a_fresh_repo_with_no_commits_reports_no_history() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        let result = scan(dir.path()).await;
        assert_eq!(result.status, HistorySecretScanStatus::NoHistory);
    }

    #[tokio::test]
    async fn a_clean_history_reports_no_findings() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        commit_file(dir.path(), "main.rs", "fn main() {}\n", "add main").await;

        let result = scan(dir.path()).await;
        assert_eq!(result.status, HistorySecretScanStatus::Ok);
        assert_eq!(result.commits_scanned, 1);
        assert!(result.findings.is_empty());
    }

    /// The whole point: a secret added in one commit and removed in the
    /// next is gone from the working tree, but not from history.
    #[tokio::test]
    async fn a_secret_committed_then_removed_is_still_found() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        commit_file(
            dir.path(),
            ".env",
            "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456\n",
            "oops, committed a real key",
        )
        .await;
        commit_file(dir.path(), ".env", "PORT=3000\n", "remove the key").await;

        // On-disk scan would find nothing — proves this is a real gap, not
        // a redundant check.
        let disk_scan = crate::secrets::scan(dir.path());
        assert!(disk_scan.findings.is_empty());

        let result = scan(dir.path()).await;
        assert_eq!(result.status, HistorySecretScanStatus::Ok);
        assert_eq!(result.commits_scanned, 2);
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].file, ".env");
        assert_eq!(result.findings[0].kind, "generic-sk-api-key");
    }

    /// Context lines (unchanged, prefixed with a space) must not cause the
    /// same secret to be reported again by a later, unrelated commit that
    /// merely touches a nearby line.
    #[tokio::test]
    async fn an_unrelated_later_commit_does_not_re_report_a_context_line() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        commit_file(
            dir.path(),
            ".env",
            "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456\nPORT=3000\n",
            "add key and port",
        )
        .await;
        commit_file(
            dir.path(),
            ".env",
            "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456\nPORT=4000\n",
            "bump port only",
        )
        .await;

        let result = scan(dir.path()).await;
        assert_eq!(
            result.findings.len(),
            1,
            "the key line is unchanged context in the second commit, not an addition: {:?}",
            result.findings
        );
    }

    #[tokio::test]
    async fn findings_never_carry_the_matched_text_itself() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path()).await;
        commit_file(
            dir.path(),
            ".env",
            "DATABASE_PASSWORD=correcthorsebatterystaple\n",
            "add password",
        )
        .await;

        let result = scan(dir.path()).await;
        assert!(!format!("{result:?}").contains("correcthorsebatterystaple"));
    }

    #[test]
    fn added_lines_strips_the_marker_but_not_plus_plus_plus_header() {
        let body = "--- a/x\n+++ b/x\n@@ -1 +1,2 @@\n unchanged\n+new line\n-old line\n";
        assert_eq!(added_lines(body), "new line");
    }

    #[test]
    fn split_file_diffs_finds_two_files_in_one_commit() {
        let diff = "diff --git a/one.txt b/one.txt\nindex 111..222 100644\n--- a/one.txt\n+++ b/one.txt\n@@ -0,0 +1 @@\n+hello\ndiff --git a/two.txt b/two.txt\nindex 333..444 100644\n--- a/two.txt\n+++ b/two.txt\n@@ -0,0 +1 @@\n+world\n";
        let files = split_file_diffs(diff);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "one.txt");
        assert_eq!(files[1].path, "two.txt");
    }
}
