//! GitHub targets discovered from a project's git remotes, via the user's own
//! `git` executable — the same choice `devos-git` makes, for the same reason:
//! the remotes the CLI reports are the ones the user actually has, including
//! anything set by their config or an include.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::github::is_safe_segment;
use crate::{IssueError, IssueResult, IssueTarget};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Hosts whose repositories this feature can file against. Deliberately an
/// exact match and not a suffix one: `github.mycorp.com` is GitHub
/// Enterprise, whose API lives at a different base URL entirely, so treating
/// it as github.com would send the token — and the issue — to the wrong
/// place. A host this does not recognise produces no target at all rather
/// than a guess, because a wrong guess here files somebody's screenshot into
/// a repository they did not choose.
const GITHUB_HOSTS: [&str; 2] = ["github.com", "www.github.com"];

/// Every GitHub repository among `repo`'s git remotes, one entry per distinct
/// `owner/name`. A project with no GitHub remote — or no repository at all —
/// has no targets, which is an ordinary state for the picker rather than a
/// failure to report.
pub async fn github_targets(repo: &Path) -> IssueResult<Vec<IssueTarget>> {
    let mut cmd = Command::new("git");
    cmd.arg("--no-optional-locks")
        .args(["remote", "-v"])
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Don't flash console windows on Windows.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let output = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| IssueError::Git("git remote timed out".into()))?
        .map_err(|e| IssueError::Git(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("not a git repository") {
            return Ok(Vec::new());
        }
        return Err(IssueError::Git(stderr));
    }

    Ok(parse_remotes(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse `git remote -v` output:
///
/// ```text
/// origin<TAB>https://github.com/owner/repo.git (fetch)
/// origin<TAB>https://github.com/owner/repo.git (push)
/// ```
///
/// Every remote appears twice, so results are deduplicated by `owner/name`.
/// A line this cannot make sense of is skipped rather than failing the list —
/// one odd remote must not cost the user the others.
fn parse_remotes(output: &str) -> Vec<IssueTarget> {
    let mut targets: Vec<IssueTarget> = Vec::new();
    for line in output.lines() {
        let mut fields = line.split_whitespace();
        let (Some(remote), Some(url)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Some((owner, name)) = parse_remote_url(url) else {
            continue;
        };
        // GitHub treats owner and repository names case-insensitively, so two
        // remotes spelled differently are still one target.
        let already = targets.iter().any(|target| {
            target.owner.eq_ignore_ascii_case(&owner) && target.name.eq_ignore_ascii_case(&name)
        });
        if !already {
            targets.push(IssueTarget {
                owner,
                name,
                remote: remote.to_string(),
            });
        }
    }
    targets
}

/// `owner` and `name` from one clone URL, or `None` if it is not a GitHub
/// repository. Both forms git writes have to work, and they are structurally
/// different: `https://github.com/owner/repo.git` separates host from path
/// with `/`, while the scp-like `git@github.com:owner/repo.git` has no scheme
/// and separates them with `:`.
fn parse_remote_url(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    // `https://`, `ssh://`, `git://` — everything after the scheme is shaped
    // alike; the scp form has no scheme and falls through unchanged.
    let (had_scheme, rest) = match url.split_once("://") {
        Some((_, rest)) => (true, rest),
        None => (false, url),
    };

    // Userinfo: `git@`, and also the `x-access-token:<pat>@` form a CI
    // checkout leaves in a clone. It can contain `:` itself, so the test for
    // "is this still the authority" is the first `/`, not the first `:`.
    let rest = match rest.find('@') {
        Some(at) if !rest[..at].contains('/') => &rest[at + 1..],
        _ => rest,
    };

    // The first `/` or `:` ends the host; a bare host with neither is not a
    // repository URL.
    let idx = rest.find(['/', ':'])?;
    let (host, path) = rest.split_at(idx);
    if !GITHUB_HOSTS
        .iter()
        .any(|known| host.eq_ignore_ascii_case(known))
    {
        return None;
    }

    let scheme_port = had_scheme && path.starts_with(':');
    let mut path = path.trim_start_matches([':', '/']);
    // `ssh://git@github.com:22/owner/repo` puts a port where the owner would
    // otherwise be. Only a scheme'd URL can carry one — in the scp form the
    // `:` introduces the path — so the scp `git@github.com:owner/repo` of a
    // user whose name is all digits is not misread as a port.
    if scheme_port {
        if let Some((head, tail)) = path.split_once('/') {
            if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) {
                path = tail;
            }
        }
    }

    // `…/repo.git`, `…/repo/` and `…/repo.git/` all name the same repository.
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let path = path.trim_end_matches('/');

    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let owner = segments.next()?;
    let name = segments.next()?;
    // A clone URL is exactly two segments deep. Anything longer is a link to
    // a file or a branch that someone pasted into a remote, and guessing
    // which two segments were meant is how you file into the wrong repo.
    if segments.next().is_some() {
        return None;
    }
    // The same allowlist the URL builder trusts, applied at the point the
    // value enters the system rather than only where it is used.
    if !is_safe_segment(owner) || !is_safe_segment(name) {
        return None;
    }

    Some((owner.to_string(), name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_url_forms_yield_the_same_owner_and_name() {
        let output = "origin\thttps://github.com/devos/dev-tools.git (fetch)\n\
                      origin\thttps://github.com/devos/dev-tools.git (push)\n";
        let https = parse_remotes(output);
        assert_eq!(https.len(), 1);
        assert_eq!(https[0].owner, "devos");
        assert_eq!(https[0].name, "dev-tools");

        let output = "origin\tgit@github.com:devos/dev-tools.git (fetch)\n\
                      origin\tgit@github.com:devos/dev-tools.git (push)\n";
        let ssh = parse_remotes(output);
        assert_eq!(ssh.len(), 1);
        assert_eq!(
            (ssh[0].owner.as_str(), ssh[0].name.as_str()),
            ("devos", "dev-tools"),
            "the scp form separates host from path with `:`, not `/`"
        );
    }

    #[test]
    fn dot_git_and_trailing_slashes_are_stripped() {
        for url in [
            "https://github.com/devos/dev-tools.git",
            "https://github.com/devos/dev-tools",
            "https://github.com/devos/dev-tools/",
            "https://github.com/devos/dev-tools.git/",
            "http://github.com/devos/dev-tools.git",
            "git@github.com:devos/dev-tools.git",
            "git@github.com:devos/dev-tools",
            "ssh://git@github.com/devos/dev-tools.git",
            "ssh://git@github.com:22/devos/dev-tools.git",
            "git://github.com/devos/dev-tools.git",
            "https://www.github.com/devos/dev-tools.git",
            "https://x-access-token:ghp_notarealtoken@github.com/devos/dev-tools.git",
        ] {
            assert_eq!(
                parse_remote_url(url),
                Some(("devos".into(), "dev-tools".into())),
                "{url} did not resolve to devos/dev-tools"
            );
        }
    }

    #[test]
    fn fetch_and_push_lines_collapse_into_one_target() {
        let output = "origin\thttps://github.com/devos/dev-tools.git (fetch)\n\
                      origin\thttps://github.com/devos/dev-tools.git (push)\n\
                      upstream\tgit@github.com:upstream-org/dev-tools.git (fetch)\n\
                      upstream\tgit@github.com:upstream-org/dev-tools.git (push)\n";

        let targets = parse_remotes(output);

        assert_eq!(targets.len(), 2, "four lines, two repositories");
        assert_eq!(targets[0].remote, "origin");
        assert_eq!(targets[0].owner, "devos");
        assert_eq!(targets[1].remote, "upstream");
        assert_eq!(
            targets[1].owner, "upstream-org",
            "a fork and its parent are different targets"
        );
    }

    #[test]
    fn one_repository_reached_two_ways_is_still_one_target() {
        // The same repo as an https fetch URL and an ssh push URL, differing
        // in case — GitHub does not distinguish them, so neither does this.
        let output = "origin\thttps://github.com/DevOS/Dev-Tools.git (fetch)\n\
                      origin\tgit@github.com:devos/dev-tools.git (push)\n";

        let targets = parse_remotes(output);

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].owner, "DevOS",
            "the first spelling seen is the one shown"
        );
    }

    #[test]
    fn non_github_hosts_are_ignored_rather_than_guessed() {
        let output = "origin\tgit@gitlab.com:devos/dev-tools.git (fetch)\n\
                      origin\tgit@gitlab.com:devos/dev-tools.git (push)\n\
                      azure\thttps://dev.azure.com/devos/_git/dev-tools (fetch)\n\
                      bb\tgit@bitbucket.org:devos/dev-tools.git (fetch)\n\
                      ghe\thttps://github.mycorp.com/devos/dev-tools.git (fetch)\n\
                      gist\thttps://gist.github.com/devos/9c1a4f.git (fetch)\n\
                      sourcehut\thttps://git.sr.ht/~devos/dev-tools (fetch)\n";

        assert!(
            parse_remotes(output).is_empty(),
            "a GitLab or Enterprise remote must not produce a bogus GitHub target"
        );
    }

    #[test]
    fn a_github_remote_alongside_others_is_still_found() {
        let output = "gitlab\tgit@gitlab.com:devos/mirror.git (fetch)\n\
                      github\thttps://github.com/devos/dev-tools.git (fetch)\n\
                      github\thttps://github.com/devos/dev-tools.git (push)\n";

        let targets = parse_remotes(output);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].remote, "github");
        assert_eq!(targets[0].name, "dev-tools");
    }

    #[test]
    fn empty_input_is_an_empty_list_not_an_error() {
        for output in ["", "\n", "   \n\t\n"] {
            assert!(parse_remotes(output).is_empty(), "{output:?}");
        }
    }

    #[test]
    fn malformed_lines_are_skipped_without_panicking() {
        let output = "origin\n\
                      \n\
                      origin\tnot-a-url (fetch)\n\
                      origin\thttps:// (fetch)\n\
                      origin\thttps://github.com (fetch)\n\
                      origin\thttps://github.com/ (fetch)\n\
                      origin\thttps://github.com/lonely-owner (fetch)\n\
                      origin\thttps://github.com/owner/repo/tree/main (fetch)\n\
                      origin\thttps://github.com/../../etc/passwd (fetch)\n\
                      \t\t\n\
                      good\thttps://github.com/devos/dev-tools.git (fetch)\n";

        let targets = parse_remotes(output);

        assert_eq!(
            targets.len(),
            1,
            "only the well-formed line survives: {targets:?}"
        );
        assert_eq!(targets[0].remote, "good");
    }

    #[test]
    fn a_url_that_could_escape_a_path_is_never_a_target() {
        for url in [
            "https://github.com/../../users/someone",
            "https://github.com/owner/..",
            "https://github.com/../repo",
            "https://github.com/owner/repo%2f..%2fother",
        ] {
            assert_eq!(parse_remote_url(url), None, "{url} produced a target");
        }
    }

    mod integration {
        use super::super::*;

        async fn git(repo: &Path, args: &[&str]) {
            let status = Command::new("git")
                .args(args)
                .current_dir(repo)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .expect("run git");
            assert!(status.success(), "git {args:?} failed");
        }

        #[tokio::test]
        async fn a_real_repository_reports_only_its_github_remotes() {
            let dir = tempfile::tempdir().expect("tempdir");
            let repo = dir.path();
            git(repo, &["init", "-b", "main"]).await;
            git(
                repo,
                &[
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/devos/dev-tools.git",
                ],
            )
            .await;
            git(
                repo,
                &["remote", "add", "mirror", "git@gitlab.com:devos/mirror.git"],
            )
            .await;

            let targets = github_targets(repo).await.expect("remotes listed");

            assert_eq!(targets.len(), 1, "the GitLab mirror is not a target");
            assert_eq!(targets[0].owner, "devos");
            assert_eq!(targets[0].name, "dev-tools");
            assert_eq!(targets[0].remote, "origin");
        }

        #[tokio::test]
        async fn a_repository_with_no_remotes_has_no_targets() {
            let dir = tempfile::tempdir().expect("tempdir");
            git(dir.path(), &["init", "-b", "main"]).await;

            assert!(github_targets(dir.path()).await.unwrap().is_empty());
        }

        #[tokio::test]
        async fn a_directory_that_is_not_a_repository_is_not_an_error() {
            let dir = tempfile::tempdir().expect("tempdir");

            let targets = github_targets(dir.path())
                .await
                .expect("a plain folder has no targets, and that is not a failure");

            assert!(targets.is_empty());
        }
    }
}
