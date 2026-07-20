//! Git operations built on the CLI wrapper. All parsing lives here with
//! tests; commands stay dumb.

use std::path::Path;

use crate::cli::{run_git, GitError, GitResult};
use crate::types::{GitBranch, GitCommit, GitFileEntry, GitRepoInfo};

const FIELD_SEP: char = '\u{1f}';

pub async fn repo_info(repo: &Path) -> GitResult<GitRepoInfo> {
    match run_git(repo, &["status", "--porcelain=v2", "--branch", "-z"]).await {
        Ok(out) => Ok(parse_status(&out).0),
        Err(GitError::NotARepo(_)) => Ok(GitRepoInfo {
            is_repo: false,
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
        }),
        Err(e) => Err(e),
    }
}

pub async fn status(repo: &Path) -> GitResult<(GitRepoInfo, Vec<GitFileEntry>)> {
    let out = run_git(repo, &["status", "--porcelain=v2", "--branch", "-z"]).await?;
    Ok(parse_status(&out))
}

pub async fn stage(repo: &Path, files: &[String]) -> GitResult<()> {
    let mut args = vec!["add", "--"];
    args.extend(files.iter().map(String::as_str));
    run_git(repo, &args).await.map(|_| ())
}

pub async fn unstage(repo: &Path, files: &[String]) -> GitResult<()> {
    let mut args = vec!["restore", "--staged", "--"];
    args.extend(files.iter().map(String::as_str));
    match run_git(repo, &args).await {
        Ok(_) => Ok(()),
        // A repo with no commits has no HEAD to restore from; drop the
        // files from the index instead.
        Err(GitError::Failed { stderr, .. }) if stderr.contains("could not resolve 'HEAD'") => {
            let mut args = vec!["rm", "--cached", "-r", "--"];
            args.extend(files.iter().map(String::as_str));
            run_git(repo, &args).await.map(|_| ())
        }
        Err(e) => Err(e),
    }
}

/// Discard worktree changes for one file. Untracked files are deleted.
pub async fn discard(repo: &Path, file: &str, untracked: bool) -> GitResult<()> {
    if untracked {
        let target = repo.join(file);
        if target.is_file() {
            std::fs::remove_file(&target)?;
        }
        Ok(())
    } else {
        run_git(repo, &["restore", "--", file]).await.map(|_| ())
    }
}

pub async fn commit(repo: &Path, message: &str) -> GitResult<String> {
    if message.trim().is_empty() {
        return Err(GitError::Failed {
            command: "commit".into(),
            stderr: "commit message is empty".into(),
        });
    }
    run_git(repo, &["commit", "-m", message]).await?;
    let id = run_git(repo, &["rev-parse", "HEAD"]).await?;
    Ok(id.trim().to_string())
}

pub async fn log(repo: &Path, limit: u32) -> GitResult<Vec<GitCommit>> {
    let format = format!("%H{FIELD_SEP}%h{FIELD_SEP}%an{FIELD_SEP}%at{FIELD_SEP}%s");
    let limit = limit.to_string();
    let result = run_git(
        repo,
        &[
            "log",
            "-z",
            &format!("--pretty=format:{format}"),
            "-n",
            &limit,
        ],
    )
    .await;
    match result {
        Ok(out) => Ok(out
            .split('\0')
            .filter(|record| !record.is_empty())
            .filter_map(parse_commit)
            .collect()),
        // A freshly initialized repo has no commits; that is not an error.
        Err(GitError::Failed { stderr, .. }) if stderr.contains("does not have any commits") => {
            Ok(Vec::new())
        }
        Err(e) => Err(e),
    }
}

pub async fn branches(repo: &Path) -> GitResult<Vec<GitBranch>> {
    let out = run_git(
        repo,
        &[
            "branch",
            "--format=%(refname:short)%09%(HEAD)%09%(upstream:short)",
        ],
    )
    .await?;
    Ok(out
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.trim().to_string();
            let head = parts.next().unwrap_or("").trim();
            let upstream = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
            Some(GitBranch {
                current: head == "*",
                name,
                upstream,
            })
        })
        .collect())
}

pub async fn switch(repo: &Path, branch: &str, create: bool) -> GitResult<()> {
    let args: Vec<&str> = if create {
        vec!["switch", "-c", branch]
    } else {
        vec!["switch", branch]
    };
    run_git(repo, &args).await.map(|_| ())
}

pub async fn push(repo: &Path) -> GitResult<String> {
    run_git(repo, &["push"]).await
}

pub async fn pull(repo: &Path) -> GitResult<String> {
    run_git(repo, &["pull"]).await
}

const MAX_DIFF_BYTES: usize = 512 * 1024;

/// Full staged diff, capped for prompt use (AI commit messages).
pub async fn staged_diff(repo: &Path, max_bytes: usize) -> GitResult<String> {
    let diff = run_git(repo, &["diff", "--cached"]).await?;
    if diff.len() > max_bytes {
        // Cut on a char boundary, then back to the last full line.
        let mut end = max_bytes;
        while end > 0 && !diff.is_char_boundary(end) {
            end -= 1;
        }
        let cut = &diff[..end];
        let cut = cut.rfind('\n').map(|i| &cut[..i]).unwrap_or(cut);
        Ok(format!("{cut}\n… diff truncated …\n"))
    } else {
        Ok(diff)
    }
}

/// Unified diff for one file. Untracked files render as a synthesized
/// all-additions diff so the viewer treats every file the same way.
pub async fn diff_file(
    repo: &Path,
    file: &str,
    staged: bool,
    untracked: bool,
) -> GitResult<String> {
    if untracked {
        let content = std::fs::read(repo.join(file))?;
        if content.contains(&0) {
            return Ok(format!("Binary file: {file}"));
        }
        let text = String::from_utf8_lossy(&content);
        let lines: Vec<&str> = text.lines().collect();
        let mut diff = format!(
            "--- /dev/null\n+++ b/{file}\n@@ -0,0 +1,{} @@\n",
            lines.len()
        );
        for line in lines {
            diff.push('+');
            diff.push_str(line);
            diff.push('\n');
        }
        return Ok(truncate_diff(diff));
    }

    let args: Vec<&str> = if staged {
        vec!["diff", "--cached", "--", file]
    } else {
        vec!["diff", "--", file]
    };
    run_git(repo, &args).await.map(truncate_diff)
}

fn truncate_diff(diff: String) -> String {
    if diff.len() > MAX_DIFF_BYTES {
        let mut cut = diff[..MAX_DIFF_BYTES].to_string();
        cut.push_str("\n… diff truncated …\n");
        cut
    } else {
        diff
    }
}

fn parse_commit(record: &str) -> Option<GitCommit> {
    let mut fields = record.split(FIELD_SEP);
    Some(GitCommit {
        id: fields.next()?.trim_matches('\n').to_string(),
        short_id: fields.next()?.to_string(),
        author: fields.next()?.to_string(),
        time: fields.next()?.parse().ok()?,
        summary: fields.next().unwrap_or("").to_string(),
    })
}

/// Parse `git status --porcelain=v2 --branch -z` output.
fn parse_status(out: &str) -> (GitRepoInfo, Vec<GitFileEntry>) {
    let mut info = GitRepoInfo {
        is_repo: true,
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
    };
    let mut entries = Vec::new();

    let mut tokens = out.split('\0').peekable();
    while let Some(token) = tokens.next() {
        if token.is_empty() {
            continue;
        }
        if let Some(header) = token.strip_prefix("# ") {
            if let Some(head) = header.strip_prefix("branch.head ") {
                if head != "(detached)" {
                    info.branch = Some(head.to_string());
                }
            } else if let Some(up) = header.strip_prefix("branch.upstream ") {
                info.upstream = Some(up.to_string());
            } else if let Some(ab) = header.strip_prefix("branch.ab ") {
                for part in ab.split(' ') {
                    if let Some(a) = part.strip_prefix('+') {
                        info.ahead = a.parse().unwrap_or(0);
                    } else if let Some(b) = part.strip_prefix('-') {
                        info.behind = b.parse().unwrap_or(0);
                    }
                }
            }
            continue;
        }

        let kind = token.chars().next().unwrap_or(' ');
        match kind {
            '1' | '2' | 'u' => {
                // "<type> <XY> <...> <path>"; fields are space-separated,
                // the path is everything after the fixed field count.
                let field_count = match kind {
                    '1' => 8,
                    '2' => 9,
                    _ => 10,
                };
                let mut rest = token.splitn(field_count + 1, ' ');
                let _type = rest.next();
                let xy = rest.next().unwrap_or("..");
                let mut skip = rest;
                // After consuming type + XY, the path sits at index
                // field_count - 2 of the remaining fields (nth is 0-based).
                let path = skip.nth(field_count - 2).unwrap_or("").to_string();
                let mut chars = xy.chars();
                let x = chars.next().unwrap_or('.').to_string();
                let y = chars.next().unwrap_or('.').to_string();
                let orig_path = if kind == '2' {
                    tokens.next().map(String::from)
                } else {
                    None
                };
                entries.push(GitFileEntry {
                    path,
                    orig_path,
                    staged: if kind == 'u' { ".".into() } else { x },
                    unstaged: if kind == 'u' { ".".into() } else { y },
                    untracked: false,
                    conflicted: kind == 'u',
                });
            }
            '?' => {
                entries.push(GitFileEntry {
                    path: token[2..].to_string(),
                    orig_path: None,
                    staged: ".".into(),
                    unstaged: ".".into(),
                    untracked: true,
                    conflicted: false,
                });
            }
            _ => {}
        }
    }

    (info, entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_headers() {
        let out = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0";
        let (info, entries) = parse_status(out);
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert_eq!(info.upstream.as_deref(), Some("origin/main"));
        assert_eq!(info.ahead, 2);
        assert_eq!(info.behind, 1);
        assert!(entries.is_empty());
    }

    #[test]
    fn parses_ordinary_untracked_and_rename_entries() {
        let out = "1 M. N... 100644 100644 100644 abc def src/lib.rs\0\
                   ? new.txt\0\
                   2 R. N... 100644 100644 100644 abc def R100 new_name.rs\0old_name.rs\0";
        let (_, entries) = parse_status(out);
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].path, "src/lib.rs");
        assert_eq!(entries[0].staged, "M");
        assert!(entries[0].has_staged_changes());
        assert!(!entries[0].has_unstaged_changes());

        assert!(entries[1].untracked);
        assert_eq!(entries[1].path, "new.txt");
        assert!(entries[1].has_unstaged_changes());

        assert_eq!(entries[2].path, "new_name.rs");
        assert_eq!(entries[2].orig_path.as_deref(), Some("old_name.rs"));
        assert_eq!(entries[2].staged, "R");
    }

    #[test]
    fn parses_paths_with_spaces() {
        let out = "1 .M N... 100644 100644 100644 abc def my folder/my file.txt\0";
        let (_, entries) = parse_status(out);
        assert_eq!(entries[0].path, "my folder/my file.txt");
        assert_eq!(entries[0].unstaged, "M");
    }

    mod integration {
        use super::super::*;
        use tempfile::TempDir;

        async fn init_repo() -> TempDir {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path();
            run_git(path, &["init", "-b", "main"])
                .await
                .expect("git init");
            run_git(path, &["config", "user.email", "dev@devos.local"])
                .await
                .expect("config email");
            run_git(path, &["config", "user.name", "DevOS Test"])
                .await
                .expect("config name");
            dir
        }

        #[tokio::test]
        async fn full_workflow_status_stage_commit_log() {
            let dir = init_repo().await;
            let repo = dir.path();

            std::fs::write(repo.join("hello.txt"), "one\n").unwrap();
            let (info, entries) = status(repo).await.unwrap();
            assert!(info.is_repo);
            assert_eq!(info.branch.as_deref(), Some("main"));
            assert_eq!(entries.len(), 1);
            assert!(entries[0].untracked);

            let diff = diff_file(repo, "hello.txt", false, true).await.unwrap();
            assert!(diff.contains("+one"));

            stage(repo, &["hello.txt".into()]).await.unwrap();
            let (_, entries) = status(repo).await.unwrap();
            assert_eq!(entries[0].staged, "A");

            unstage(repo, &["hello.txt".into()]).await.unwrap();
            let (_, entries) = status(repo).await.unwrap();
            assert!(entries[0].untracked);

            stage(repo, &["hello.txt".into()]).await.unwrap();

            let sdiff = staged_diff(repo, 64 * 1024).await.unwrap();
            assert!(sdiff.contains("+one"), "staged diff has the addition");
            let tiny = staged_diff(repo, 16).await.unwrap();
            assert!(tiny.contains("truncated"), "oversized diff is truncated");

            let id = commit(repo, "feat: first commit").await.unwrap();
            assert_eq!(id.len(), 40);

            let commits = log(repo, 10).await.unwrap();
            assert_eq!(commits.len(), 1);
            assert_eq!(commits[0].summary, "feat: first commit");
            assert_eq!(commits[0].id, id);

            let (_, entries) = status(repo).await.unwrap();
            assert!(entries.is_empty(), "clean after commit");
        }

        #[tokio::test]
        async fn branches_switch_and_discard() {
            let dir = init_repo().await;
            let repo = dir.path();
            std::fs::write(repo.join("a.txt"), "base\n").unwrap();
            stage(repo, &["a.txt".into()]).await.unwrap();
            commit(repo, "base").await.unwrap();

            switch(repo, "feature/x", true).await.unwrap();
            let list = branches(repo).await.unwrap();
            let current = list.iter().find(|b| b.current).unwrap();
            assert_eq!(current.name, "feature/x");
            assert_eq!(list.len(), 2);

            // Modify + discard restores the committed content.
            std::fs::write(repo.join("a.txt"), "changed\n").unwrap();
            let diff = diff_file(repo, "a.txt", false, false).await.unwrap();
            assert!(diff.contains("-base") && diff.contains("+changed"));
            discard(repo, "a.txt", false).await.unwrap();
            let (_, entries) = status(repo).await.unwrap();
            assert!(entries.is_empty());

            // Discarding an untracked file deletes it.
            std::fs::write(repo.join("junk.txt"), "x").unwrap();
            discard(repo, "junk.txt", true).await.unwrap();
            assert!(!repo.join("junk.txt").exists());
        }

        #[tokio::test]
        async fn empty_repo_log_is_empty_and_non_repo_detected() {
            let dir = init_repo().await;
            assert!(log(dir.path(), 10).await.unwrap().is_empty());

            let plain = tempfile::tempdir().unwrap();
            let info = repo_info(plain.path()).await.unwrap();
            assert!(!info.is_repo);
        }
    }
}
