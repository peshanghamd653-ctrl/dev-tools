//! The GitHub REST client: one POST, and the error taxonomy that makes its
//! failures actionable.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::{CreatedIssue, IssueError, IssueResult};

/// The real API. Every function still takes the base URL as a parameter — the
/// app passes this constant, tests pass a local socket, and no test in this
/// crate can accidentally reach the network.
pub const DEFAULT_BASE_URL: &str = "https://api.github.com";

/// GitHub answers a request with no `User-Agent` with a 403 and an
/// administrative-rules message, so this is not decoration — without it the
/// feature fails on every call.
const USER_AGENT: &str = concat!("DevOS/", env!("CARGO_PKG_VERSION"));

/// GitHub asks callers to pin the REST version explicitly; unpinned requests
/// follow whatever the current default is, which is a silent breakage waiting
/// for a release we would not hear about.
const API_VERSION: &str = "2022-11-28";

const TIMEOUT: Duration = Duration::from_secs(20);

/// Error bodies are echoed to the user, so they are capped: an HTML error
/// page from a proxy in front of the API would otherwise land whole in a
/// toast.
const MAX_ERROR_BODY_CHARS: usize = 512;

/// File one issue. `owner` and `name` are interpolated into the path, so they
/// are validated first rather than escaped — see [`is_safe_segment`].
pub async fn create_issue(
    token: &str,
    base_url: &str,
    owner: &str,
    name: &str,
    title: &str,
    body: &str,
) -> IssueResult<CreatedIssue> {
    // Checked here and not only in the command layer: this is the function
    // that would otherwise send an anonymous request and get back a 401,
    // reporting "your token is bad" for a token that was never set.
    if token.trim().is_empty() {
        return Err(IssueError::NotConfigured);
    }
    if owner.trim().is_empty() || name.trim().is_empty() {
        return Err(IssueError::Invalid(
            "choose a repository to file against".into(),
        ));
    }
    // GitHub accepts an issue with an empty body but not one with an empty
    // title, and refusing it here costs a round trip less than a 422.
    if title.trim().is_empty() {
        return Err(IssueError::Invalid("the issue needs a title".into()));
    }
    if !is_safe_segment(owner) || !is_safe_segment(name) {
        return Err(IssueError::Invalid(format!(
            "{owner}/{name} is not a repository name"
        )));
    }

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| IssueError::Request(e.to_string()))?;

    let url = format!(
        "{}/repos/{owner}/{name}/issues",
        base_url.trim_end_matches('/')
    );
    let response = client
        .post(&url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", API_VERSION)
        .json(&json!({ "title": title, "body": body }))
        .send()
        .await
        .map_err(|e| IssueError::Request(e.to_string()))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| IssueError::Request(e.to_string()))?;

    if !status.is_success() {
        return Err(classify(status.as_u16(), &text));
    }

    let raw: RawIssue =
        serde_json::from_str(&text).map_err(|e| IssueError::Decode(e.to_string()))?;
    Ok(CreatedIssue {
        number: raw.number.unwrap_or_default(),
        url: raw.html_url.unwrap_or_default(),
        title: raw.title.unwrap_or_default(),
    })
}

fn classify(status: u16, body: &str) -> IssueError {
    match status {
        401 | 403 => IssueError::Auth { status },
        404 => IssueError::NotFound,
        _ => IssueError::Api {
            status,
            body: truncate(body),
        },
    }
}

/// Whether a string can be interpolated into the URL path as-is. GitHub owner
/// and repository names are `[A-Za-z0-9._-]`, so an allowlist is both the
/// accurate rule and the one that makes `/` and `%2f` impossible; `..` is
/// spelled out separately because its characters are individually legal.
pub(crate) fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != ".."
        && !segment.contains("..")
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn truncate(body: &str) -> String {
    let body = body.trim();
    if body.chars().count() <= MAX_ERROR_BODY_CHARS {
        return body.to_string();
    }
    let mut out: String = body.chars().take(MAX_ERROR_BODY_CHARS).collect();
    out.push('…');
    out
}

// Every field is optional deliberately: a 201 that is missing one of them is
// still a created issue, and losing the link is better than reporting a
// failure for something that already happened on GitHub's side.
#[derive(Deserialize)]
struct RawIssue {
    number: Option<i64>,
    html_url: Option<String>,
    title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TOKEN: &str = "devos-test-token-abc123";

    const CREATED_JSON: &str = r#"{
        "number": 4123,
        "html_url": "https://github.com/devos/dev-tools/issues/4123",
        "title": "Palette closes on the second keystroke",
        "state": "open"
    }"#;

    /// A one-shot local HTTP server: accepts a single connection, captures the
    /// raw request, writes a canned response. The same technique as
    /// `devos-deploy` and `devos-api`, with one addition — it keeps reading
    /// until the declared `content-length` has arrived, because these tests
    /// assert on the POST body and a single `read` is only guaranteed to
    /// return the headers.
    async fn one_shot_json(status: &str, body: &str) -> (String, tokio::task::JoinHandle<String>) {
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
                if request_is_complete(&request) {
                    break;
                }
            }
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    /// Headers plus as many body bytes as `content-length` promised.
    fn request_is_complete(request: &[u8]) -> bool {
        let text = String::from_utf8_lossy(request);
        let Some(head_end) = text.find("\r\n\r\n") else {
            return false;
        };
        let declared = text[..head_end]
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if !name.eq_ignore_ascii_case("content-length") {
                    return None;
                }
                value.trim().parse::<usize>().ok()
            })
            .unwrap_or(0);
        request.len() >= head_end + 4 + declared
    }

    #[tokio::test]
    async fn a_created_issue_is_parsed() {
        let (base, server) = one_shot_json("201 Created", CREATED_JSON).await;

        let issue = create_issue(
            TOKEN,
            &base,
            "devos",
            "dev-tools",
            "Palette closes on the second keystroke",
            "steps to reproduce…",
        )
        .await
        .expect("issue created");

        assert_eq!(issue.number, 4123);
        assert_eq!(
            issue.url, "https://github.com/devos/dev-tools/issues/4123",
            "the frontend opens `html_url`, not the API url"
        );
        assert_eq!(issue.title, "Palette closes on the second keystroke");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn the_request_carries_the_token_the_user_agent_and_the_issue_body() {
        let (base, server) = one_shot_json("201 Created", CREATED_JSON).await;

        create_issue(
            TOKEN,
            &base,
            "devos",
            "dev-tools",
            "Palette closes on the second keystroke",
            "It happens every time on Windows 11.",
        )
        .await
        .unwrap();

        let raw = server.await.unwrap();
        let lower = raw.to_lowercase();
        assert!(
            raw.starts_with("POST /repos/devos/dev-tools/issues"),
            "raw request was {raw:?}"
        );
        // Header names go out lowercase on the wire; the values do not.
        assert!(
            lower.contains("authorization: bearer"),
            "no bearer scheme in the bytes the server received: {raw:?}"
        );
        assert!(
            raw.contains(TOKEN),
            "the token never reached the wire: {raw:?}"
        );
        assert!(
            lower.contains("user-agent: devos/"),
            "GitHub 403s a request with no user-agent: {raw:?}"
        );
        assert!(
            lower.contains("accept: application/vnd.github+json"),
            "raw request was {raw:?}"
        );
        assert!(
            lower.contains("x-github-api-version: 2022-11-28"),
            "raw request was {raw:?}"
        );
        // The body itself, asserted on the bytes rather than on a builder.
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        let json: serde_json::Value = serde_json::from_str(body)
            .unwrap_or_else(|e| panic!("body was not JSON ({e}): {body:?}"));
        assert_eq!(json["title"], "Palette closes on the second keystroke");
        assert_eq!(json["body"], "It happens every time on Windows 11.");
    }

    #[tokio::test]
    async fn an_unauthorized_response_is_an_auth_error() {
        let (base, server) =
            one_shot_json("401 Unauthorized", r#"{"message":"Bad credentials"}"#).await;

        let error = create_issue(TOKEN, &base, "devos", "dev-tools", "t", "b")
            .await
            .unwrap_err();

        assert!(
            matches!(error, IssueError::Auth { status: 401 }),
            "a bad token must be distinguishable from a generic failure: {error:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_forbidden_response_is_also_an_auth_error() {
        let (base, server) = one_shot_json(
            "403 Forbidden",
            r#"{"message":"Resource not accessible by personal access token"}"#,
        )
        .await;

        let error = create_issue(TOKEN, &base, "devos", "dev-tools", "t", "b")
            .await
            .unwrap_err();

        assert!(
            matches!(error, IssueError::Auth { status: 403 }),
            "a token without `issues:write` is an auth problem: {error:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_missing_repository_is_its_own_error_not_a_generic_api_failure() {
        let (base, server) = one_shot_json("404 Not Found", r#"{"message":"Not Found"}"#).await;

        let error = create_issue(TOKEN, &base, "devos", "private-thing", "t", "b")
            .await
            .unwrap_err();

        // The common cause is a private repo the token cannot see, which is a
        // different fix from a bad token and a different fix again from a
        // server-side failure — so it may not collapse into `Api`.
        assert!(
            matches!(error, IssueError::NotFound),
            "expected NotFound, got {error:?}"
        );
        assert!(
            error.to_string().contains("allowed to see it"),
            "the message must point at access, not spelling: {error}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_rejected_issue_keeps_its_status_and_body() {
        let (base, server) = one_shot_json(
            "422 Unprocessable Entity",
            r#"{"message":"Validation Failed","errors":[{"resource":"Issue","field":"title","code":"missing_field"}]}"#,
        )
        .await;

        match create_issue(TOKEN, &base, "devos", "dev-tools", "t", "b")
            .await
            .unwrap_err()
        {
            IssueError::Api { status, body } => {
                assert_eq!(
                    status, 422,
                    "the status the UI reports must be the real one"
                );
                assert!(
                    body.contains("Validation Failed"),
                    "echoed body was {body:?}"
                );
            }
            other => panic!("expected an API error, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_long_error_body_is_truncated() {
        let (base, server) = one_shot_json("502 Bad Gateway", &"x".repeat(4096)).await;

        match create_issue(TOKEN, &base, "devos", "dev-tools", "t", "b")
            .await
            .unwrap_err()
        {
            // The ellipsis is the one character allowed past the cap.
            IssueError::Api { body, .. } => assert!(
                body.chars().count() <= MAX_ERROR_BODY_CHARS + 1,
                "echoed body was {} chars",
                body.chars().count()
            ),
            other => panic!("expected an API error, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_success_carrying_junk_is_a_decode_error() {
        let (base, server) = one_shot_json("201 Created", "<html>maintenance</html>").await;

        let error = create_issue(TOKEN, &base, "devos", "dev-tools", "t", "b")
            .await
            .unwrap_err();

        assert!(
            matches!(error, IssueError::Decode(_)),
            "a 201 carrying junk is not an API error: {error:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_refused_connection_is_a_request_error() {
        // Port 1 is essentially guaranteed closed.
        let error = create_issue(TOKEN, "http://127.0.0.1:1", "devos", "dev-tools", "t", "b")
            .await
            .unwrap_err();

        assert!(
            matches!(error, IssueError::Request(_)),
            "a transport failure is not an API error: {error:?}"
        );
    }

    /// Every rejection below points at a closed port, so reaching the network
    /// at all would surface as `Request` — anything else is positive proof
    /// that no request was attempted.
    const CLOSED: &str = "http://127.0.0.1:1";

    #[tokio::test]
    async fn a_blank_token_is_not_configured_and_sends_nothing() {
        for token in ["", "   "] {
            let error = create_issue(token, CLOSED, "devos", "dev-tools", "t", "b")
                .await
                .unwrap_err();
            assert!(
                matches!(error, IssueError::NotConfigured),
                "token {token:?} produced {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_blank_owner_name_or_title_is_rejected_before_any_request() {
        for (owner, name, title) in [
            ("", "dev-tools", "a title"),
            ("   ", "dev-tools", "a title"),
            ("devos", "", "a title"),
            ("devos", "  ", "a title"),
            ("devos", "dev-tools", ""),
            ("devos", "dev-tools", "   "),
        ] {
            let error = create_issue(TOKEN, CLOSED, owner, name, title, "b")
                .await
                .unwrap_err();
            assert!(
                matches!(error, IssueError::Invalid(_)),
                "({owner:?}, {name:?}, {title:?}) produced {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_segment_that_could_escape_the_url_path_is_rejected() {
        for (owner, name) in [
            ("..", "dev-tools"),
            ("devos", ".."),
            ("devos/x", "dev-tools"),
            ("devos", "dev-tools/issues"),
            ("devos", "../../users/someone-else"),
            ("devos", "dev tools"),
            ("devos", "a%2fb"),
        ] {
            let error = create_issue(TOKEN, CLOSED, owner, name, "a title", "b")
                .await
                .unwrap_err();
            assert!(
                matches!(error, IssueError::Invalid(_)),
                "{owner}/{name} reached the network: {error:?}"
            );
        }
    }

    #[test]
    fn ordinary_repository_names_stay_acceptable() {
        // The allowlist must not be so tight that real repositories fail it.
        for segment in [
            "dev-tools",
            "devos",
            "docs.rs",
            "my_repo",
            "a",
            "repo.js",
            "x2",
        ] {
            assert!(is_safe_segment(segment), "{segment} was rejected");
        }
    }
}
