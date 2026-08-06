//! One HTTP check: did the URL answer, with what status, and how fast.

use std::time::{Duration, Instant};

use crate::{MonitorCheck, MonitorError, MonitorResult};

const TIMEOUT: Duration = Duration::from_secs(15);

/// Redirects are followed, but not indefinitely — a redirect loop should be
/// reported as a failure, not chased until the timeout.
const MAX_REDIRECTS: usize = 5;

/// GET `url` once and describe what happened.
///
/// A transport failure — DNS, refused, timeout, TLS — is a normal result
/// rather than an error: it is precisely what a monitor exists to record, so
/// it comes back as `ok = false` with no status and the message in `error`.
/// `Err` is reserved for a URL that should never have been stored.
///
/// The response body is never read. A status line and a duration are all an
/// uptime check needs, and bodies routinely carry session tokens, personal
/// data, and error dumps that have no business being written to disk.
pub async fn run_check(monitor_id: &str, url: &str) -> MonitorResult<MonitorCheck> {
    // Re-validated here and not only on the write path. This is the function
    // that actually performs the request, so it is the last place the scheme
    // guard can still hold if a row ever arrives from somewhere else.
    let url = crate::repo::validate_url(url)?;

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .build()
        .map_err(|e| MonitorError::Invalid(format!("http client: {e}")))?;

    let started = Instant::now();
    let outcome = client.get(&url).send().await;
    let duration_ms = started.elapsed().as_millis() as i64;

    let (status, ok, error) = match outcome {
        Ok(response) => {
            let status = response.status();
            // `response` is dropped here without `.bytes()` or `.text()`.
            (Some(status.as_u16()), status.is_success(), None)
        }
        Err(e) => (None, false, Some(e.to_string())),
    };

    Ok(MonitorCheck {
        id: uuid::Uuid::new_v4().to_string(),
        monitor_id: monitor_id.to_string(),
        status,
        ok,
        duration_ms,
        error,
        checked_at: chrono::Utc::now().timestamp_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot local HTTP server: accepts a single connection, captures
    /// the raw request, writes a canned response. Fully hermetic — same
    /// technique as `devos-api`.
    async fn one_shot_server(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
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
    async fn a_success_response_is_recorded_as_ok() {
        let (url, server) = one_shot_server(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 13\r\nconnection: close\r\n\r\nsecret-token!",
        )
        .await;

        let check = run_check("m1", &format!("{url}/health")).await.unwrap();

        assert_eq!(check.monitor_id, "m1");
        assert_eq!(check.status, Some(200));
        assert!(check.ok);
        assert!(check.error.is_none());
        assert!(check.duration_ms >= 0);
        assert!(check.checked_at > 0);

        let raw = server.await.unwrap();
        assert!(raw.starts_with("GET /health"), "raw request was {raw:?}");
        // The body the server sent is nowhere in the result — `MonitorCheck`
        // has no field that could hold it, which is the guarantee.
        assert!(!format!("{check:?}").contains("secret-token"));
    }

    #[tokio::test]
    async fn a_server_error_response_is_recorded_as_not_ok() {
        let (url, server) = one_shot_server(
            "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )
        .await;

        let check = run_check("m1", &url).await.unwrap();

        assert_eq!(check.status, Some(500), "a 500 still answered");
        assert!(!check.ok, "only 2xx counts as up");
        assert!(
            check.error.is_none(),
            "an HTTP error is not a transport error"
        );

        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_refused_connection_has_no_status_and_an_error() {
        // Port 1 is essentially guaranteed closed.
        let check = run_check("m1", "http://127.0.0.1:1/").await.unwrap();

        assert!(!check.ok);
        assert_eq!(
            check.status, None,
            "nothing answered, so there is no status"
        );
        assert!(
            check.error.as_deref().is_some_and(|e| !e.is_empty()),
            "transport failure must carry a message: {:?}",
            check.error
        );
    }

    #[tokio::test]
    async fn refuses_to_fetch_anything_that_is_not_http() {
        for url in ["file:///etc/passwd", "ftp://example.com", "not a url"] {
            assert!(
                matches!(run_check("m1", url).await, Err(MonitorError::Invalid(_))),
                "run_check followed {url:?}"
            );
        }
    }
}
