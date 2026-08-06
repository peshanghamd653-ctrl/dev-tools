//! The Vercel REST client: two GETs, defensive mapping, and nothing that can
//! change state on the account.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::{DeployError, DeployProject, DeployResult, Deployment};

/// The real API. Every function still takes the base URL as a parameter — the
/// app passes this constant, tests pass a local socket, and no test in this
/// crate can accidentally reach the network.
pub const DEFAULT_BASE_URL: &str = "https://api.vercel.com";

const TIMEOUT: Duration = Duration::from_secs(20);

/// How many deployments the panel shows per project.
const DEPLOYMENT_LIMIT: usize = 20;

/// Error bodies are echoed to the user, so they are capped: an HTML error
/// page from a proxy in front of the API would otherwise land whole in a
/// toast.
const MAX_ERROR_BODY_CHARS: usize = 512;

/// Projects the token can see, in the order Vercel returns them.
pub async fn list_projects(token: &str, base_url: &str) -> DeployResult<Vec<DeployProject>> {
    let body = get(token, base_url, "/v9/projects", &[]).await?;
    let envelope: ProjectsEnvelope =
        serde_json::from_str(&body).map_err(|e| DeployError::Decode(e.to_string()))?;
    Ok(envelope.projects.into_iter().map(map_project).collect())
}

/// The most recent deployments of one project, newest first.
pub async fn list_deployments(
    token: &str,
    base_url: &str,
    project_id: &str,
) -> DeployResult<Vec<Deployment>> {
    let limit = DEPLOYMENT_LIMIT.to_string();
    let body = get(
        token,
        base_url,
        "/v6/deployments",
        &[("projectId", project_id), ("limit", &limit)],
    )
    .await?;
    let envelope: DeploymentsEnvelope =
        serde_json::from_str(&body).map_err(|e| DeployError::Decode(e.to_string()))?;
    Ok(envelope
        .deployments
        .into_iter()
        .map(map_deployment)
        .collect())
}

async fn get(
    token: &str,
    base_url: &str,
    path: &str,
    query: &[(&str, &str)],
) -> DeployResult<String> {
    // Checked here and not only in the command layer: this is the function
    // that would otherwise send an anonymous request and get back a 403,
    // reporting "your token is bad" for a token that was never set.
    if token.trim().is_empty() {
        return Err(DeployError::NotConfigured);
    }

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| DeployError::Request(e.to_string()))?;

    let url = format!("{}{path}", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .bearer_auth(token)
        .query(query)
        .send()
        .await
        .map_err(|e| DeployError::Request(e.to_string()))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| DeployError::Request(e.to_string()))?;

    if status.is_success() {
        return Ok(body);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(DeployError::Auth {
            status: status.as_u16(),
        });
    }
    Err(DeployError::Api {
        status: status.as_u16(),
        body: truncate(&body),
    })
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

#[derive(Deserialize)]
struct ProjectsEnvelope {
    #[serde(default)]
    projects: Vec<RawProject>,
}

// Every field is optional and the timestamp is a `Value`, deliberately: one
// project missing a field — or sending its timestamp as a string — must not
// take the whole list down with it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProject {
    id: Option<String>,
    name: Option<String>,
    framework: Option<String>,
    updated_at: Option<Value>,
}

#[derive(Deserialize)]
struct DeploymentsEnvelope {
    #[serde(default)]
    deployments: Vec<RawDeployment>,
}

// Vercel does not name these fields the way it names a project's: the id
// arrives as `uid`, the timestamp as `created`, and the commit message hides
// one level down under `meta` — which is absent entirely when there was no
// commit, as with a CLI deploy.
#[derive(Deserialize)]
struct RawDeployment {
    uid: Option<String>,
    name: Option<String>,
    url: Option<String>,
    state: Option<String>,
    target: Option<String>,
    created: Option<Value>,
    meta: Option<RawMeta>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMeta {
    github_commit_message: Option<String>,
}

fn map_project(raw: RawProject) -> DeployProject {
    DeployProject {
        id: raw.id.unwrap_or_default(),
        name: raw.name.unwrap_or_default(),
        framework: raw.framework,
        updated_at: millis(raw.updated_at.as_ref()),
    }
}

fn map_deployment(raw: RawDeployment) -> Deployment {
    Deployment {
        id: raw.uid.unwrap_or_default(),
        name: raw.name.unwrap_or_default(),
        url: absolute_url(raw.url.as_deref()),
        // A deployment with no state is not something the UI knows how to
        // colour; naming it beats rendering an empty badge.
        state: raw.state.unwrap_or_else(|| "UNKNOWN".into()),
        target: raw.target,
        created_at: millis(raw.created.as_ref()),
        commit_message: raw.meta.and_then(|meta| meta.github_commit_message),
    }
}

/// Vercel sends epoch milliseconds as a JSON number, but a missing or
/// oddly-typed timestamp is not worth failing an entire list over: it
/// degrades to 0 and the UI shows no date for that one row.
fn millis(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|seconds| seconds as i64))
            .unwrap_or(0),
        Some(Value::String(text)) => text.parse().unwrap_or(0),
        _ => 0,
    }
}

/// Vercel returns bare hostnames ("my-app-abc123.vercel.app"). The frontend
/// renders this as a link, so the scheme is added once here rather than at
/// every display site.
fn absolute_url(raw: Option<&str>) -> String {
    let raw = raw.unwrap_or_default().trim();
    if raw.is_empty() || raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TOKEN: &str = "devos-test-token-abc123";

    const PROJECTS_JSON: &str = r#"{"projects":[
        {"id":"prj_9aB","name":"devos-web","framework":"nextjs","updatedAt":1754400000000,"accountId":"team_x"},
        {"id":"prj_7cD","name":"scratch-api","framework":null,"updatedAt":1754300000000}
    ]}"#;

    const DEPLOYMENTS_JSON: &str = r#"{"deployments":[
        {"uid":"dpl_1","name":"devos-web","url":"devos-web-git-main-team.vercel.app","state":"READY","target":"production","created":1754400000000,"meta":{"githubCommitMessage":"feat: ship the deployments panel","githubCommitRef":"main"}},
        {"uid":"dpl_2","name":"devos-web","url":"devos-web-jj3k1p.vercel.app","state":"BUILDING","target":null,"created":1754390000000},
        {"uid":"dpl_3","name":"devos-web","url":"devos-web-9zzq4t.vercel.app","state":"ERROR","created":1754380000000,"meta":{}}
    ]}"#;

    /// A one-shot local HTTP server: accepts a single connection, captures the
    /// raw request, writes a canned response. The same technique as
    /// `devos-monitor` and `devos-api`, extended to build the status line and
    /// `content-length` around a JSON body so these tests can carry realistic
    /// Vercel payloads without hand-counting bytes.
    async fn one_shot_json(status: &str, body: &str) -> (String, tokio::task::JoinHandle<String>) {
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            let n = socket.read(&mut request).await.unwrap();
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
            String::from_utf8_lossy(&request[..n]).into_owned()
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn projects_parse_including_one_with_no_framework() {
        let (base, server) = one_shot_json("200 OK", PROJECTS_JSON).await;

        let projects = list_projects(TOKEN, &base).await.expect("projects list");

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].id, "prj_9aB");
        assert_eq!(projects[0].name, "devos-web");
        assert_eq!(projects[0].framework.as_deref(), Some("nextjs"));
        assert_eq!(projects[0].updated_at, 1_754_400_000_000);
        assert_eq!(projects[1].name, "scratch-api");
        assert_eq!(
            projects[1].framework, None,
            "a null framework is absent, not the string \"null\""
        );

        let raw = server.await.unwrap();
        assert!(
            raw.starts_with("GET /v9/projects"),
            "raw request was {raw:?}"
        );
    }

    #[tokio::test]
    async fn an_empty_project_list_is_not_an_error() {
        let (base, server) = one_shot_json("200 OK", r#"{"projects":[]}"#).await;

        let projects = list_projects(TOKEN, &base)
            .await
            .expect("an account with no projects is a normal account");

        assert!(projects.is_empty());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn deployments_map_vercels_field_names() {
        let (base, server) = one_shot_json("200 OK", DEPLOYMENTS_JSON).await;

        let deployments = list_deployments(TOKEN, &base, "prj_9aB")
            .await
            .expect("deployments list");

        let first = &deployments[0];
        assert_eq!(first.id, "dpl_1", "the id arrives as `uid`");
        assert_eq!(
            first.created_at, 1_754_400_000_000,
            "the timestamp arrives as `created`"
        );
        assert_eq!(
            first.commit_message.as_deref(),
            Some("feat: ship the deployments panel"),
            "the commit message is nested under `meta`"
        );
        assert_eq!(first.name, "devos-web");
        assert_eq!(first.state, "READY");
        assert_eq!(first.target.as_deref(), Some("production"));
        assert_eq!(
            first.url, "https://devos-web-git-main-team.vercel.app",
            "Vercel omits the scheme; the frontend links this straight through"
        );

        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_deployment_with_no_meta_has_no_commit_message() {
        let (base, server) = one_shot_json("200 OK", DEPLOYMENTS_JSON).await;

        let deployments = list_deployments(TOKEN, &base, "prj_9aB")
            .await
            .expect("a deployment without `meta` must not fail the list");

        assert_eq!(deployments.len(), 3, "no row was dropped");
        // A CLI deploy has no commit at all, so Vercel sends no `meta` key.
        assert_eq!(deployments[1].id, "dpl_2");
        assert_eq!(deployments[1].commit_message, None);
        assert_eq!(deployments[1].target, None, "a preview has no target");
        assert_eq!(deployments[1].state, "BUILDING");
        // `meta` present but carrying no commit message is the same outcome,
        // and so is a deployment with no `target` key at all.
        assert_eq!(deployments[2].id, "dpl_3");
        assert_eq!(deployments[2].commit_message, None);
        assert_eq!(deployments[2].target, None);

        server.await.unwrap();
    }

    #[tokio::test]
    async fn the_request_carries_the_bearer_token() {
        let (base, server) = one_shot_json("200 OK", r#"{"projects":[]}"#).await;

        list_projects(TOKEN, &base).await.unwrap();

        let raw = server.await.unwrap();
        // Header names go out lowercase on the wire; the value does not.
        assert!(
            raw.to_lowercase().contains("authorization: bearer"),
            "no bearer scheme in the bytes the server received: {raw:?}"
        );
        assert!(
            raw.contains(TOKEN),
            "the token never reached the wire: {raw:?}"
        );
    }

    #[tokio::test]
    async fn deploy_list_sends_the_project_id_query_parameter() {
        let (base, server) = one_shot_json("200 OK", r#"{"deployments":[]}"#).await;

        list_deployments(TOKEN, &base, "prj_9aB").await.unwrap();

        let raw = server.await.unwrap();
        assert!(
            raw.starts_with("GET /v6/deployments?"),
            "raw request was {raw:?}"
        );
        assert!(raw.contains("projectId=prj_9aB"), "raw request was {raw:?}");
        assert!(raw.contains("limit=20"), "raw request was {raw:?}");
    }

    #[tokio::test]
    async fn an_unauthorized_response_is_an_auth_error() {
        let (base, server) = one_shot_json(
            "401 Unauthorized",
            r#"{"error":{"code":"forbidden","message":"Not authorized"}}"#,
        )
        .await;

        let error = list_projects(TOKEN, &base).await.unwrap_err();

        assert!(
            matches!(error, DeployError::Auth { status: 401 }),
            "an expired token must be distinguishable from a generic failure: {error:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_forbidden_response_is_also_an_auth_error() {
        let (base, server) = one_shot_json("403 Forbidden", "{}").await;

        let error = list_projects(TOKEN, &base).await.unwrap_err();

        assert!(
            matches!(error, DeployError::Auth { status: 403 }),
            "expected an auth error, got {error:?}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_server_error_keeps_its_status() {
        let (base, server) =
            one_shot_json("500 Internal Server Error", r#"{"error":"boom"}"#).await;

        match list_projects(TOKEN, &base).await.unwrap_err() {
            DeployError::Api { status, body } => {
                assert_eq!(
                    status, 500,
                    "the status the UI reports must be the real one"
                );
                assert!(body.contains("boom"), "echoed body was {body:?}");
            }
            other => panic!("expected an API error, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_long_error_body_is_truncated() {
        let (base, server) = one_shot_json("502 Bad Gateway", &"x".repeat(4096)).await;

        match list_projects(TOKEN, &base).await.unwrap_err() {
            // The ellipsis is the one character allowed past the cap.
            DeployError::Api { body, .. } => assert!(
                body.chars().count() <= MAX_ERROR_BODY_CHARS + 1,
                "echoed body was {} chars",
                body.chars().count()
            ),
            other => panic!("expected an API error, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_refused_connection_is_a_request_error() {
        // Port 1 is essentially guaranteed closed.
        let error = list_projects(TOKEN, "http://127.0.0.1:1")
            .await
            .unwrap_err();

        assert!(
            matches!(error, DeployError::Request(_)),
            "a transport failure is not an API error: {error:?}"
        );
    }

    #[tokio::test]
    async fn a_blank_token_is_not_configured_and_sends_nothing() {
        // The base URL points at a closed port, so touching the network at
        // all would surface as `Request` — `NotConfigured` is proof that no
        // request was attempted.
        for token in ["", "   "] {
            let error = list_projects(token, "http://127.0.0.1:1")
                .await
                .unwrap_err();
            assert!(
                matches!(error, DeployError::NotConfigured),
                "token {token:?} produced {error:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_unusable_timestamp_degrades_to_zero() {
        let (base, server) = one_shot_json(
            "200 OK",
            r#"{"projects":[
                {"id":"prj_1","name":"a","updatedAt":"tomorrow"},
                {"id":"prj_2","name":"b"}
            ]}"#,
        )
        .await;

        let projects = list_projects(TOKEN, &base)
            .await
            .expect("a bad timestamp must not fail the whole list");

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].updated_at, 0, "unparseable degrades to 0");
        assert_eq!(projects[1].updated_at, 0, "absent degrades to 0");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_body_that_is_not_json_is_a_decode_error() {
        let (base, server) = one_shot_json("200 OK", "<html>maintenance</html>").await;

        let error = list_projects(TOKEN, &base).await.unwrap_err();

        assert!(
            matches!(error, DeployError::Decode(_)),
            "a 200 carrying junk is not an API error: {error:?}"
        );
        server.await.unwrap();
    }
}
