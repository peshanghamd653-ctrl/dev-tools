//! Request execution: reqwest with a hard timeout, capped response body,
//! wall-clock timing.

use std::time::{Duration, Instant};

use crate::{ApiError, ApiHeader, ApiRequestSpec, ApiResponse, ApiResult};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub async fn send_request(spec: &ApiRequestSpec) -> ApiResult<ApiResponse> {
    let method: reqwest::Method = spec
        .method
        .to_uppercase()
        .parse()
        .map_err(|_| ApiError::Invalid(format!("unknown method: {}", spec.method)))?;
    let url: reqwest::Url = spec
        .url
        .parse()
        .map_err(|e| ApiError::Invalid(format!("invalid URL: {e}")))?;

    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| ApiError::Request(e.to_string()))?;

    let mut request = client.request(method, url);
    for header in &spec.headers {
        if header.name.trim().is_empty() {
            continue;
        }
        request = request.header(header.name.trim(), header.value.as_str());
    }
    if let Some(body) = &spec.body {
        if !body.is_empty() {
            request = request.body(body.clone());
        }
    }

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|e| ApiError::Request(e.to_string()))?;

    let status = response.status().as_u16();
    let headers: Vec<ApiHeader> = response
        .headers()
        .iter()
        .map(|(name, value)| ApiHeader {
            name: name.to_string(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect();

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ApiError::Request(e.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as i64;
    let size_bytes = bytes.len() as i64;
    let truncated = bytes.len() > MAX_BODY_BYTES;
    let shown = if truncated {
        &bytes[..MAX_BODY_BYTES]
    } else {
        &bytes[..]
    };

    Ok(ApiResponse {
        status,
        headers,
        body: String::from_utf8_lossy(shown).into_owned(),
        truncated,
        duration_ms,
        size_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot local HTTP server: accepts a single connection, captures
    /// the raw request, writes a canned response. Fully hermetic.
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
    async fn full_round_trip_with_headers_and_body() {
        let (url, server) = one_shot_server(
            "HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: 15\r\nconnection: close\r\n\r\n{\"ok\":\"created\"}",
        )
        .await;

        let spec = ApiRequestSpec {
            method: "post".into(),
            url: format!("{url}/things"),
            headers: vec![ApiHeader {
                name: "x-devos-test".into(),
                value: "1".into(),
            }],
            body: Some("{\"name\":\"widget\"}".into()),
        };
        let response = send_request(&spec).await.expect("request succeeds");

        assert_eq!(response.status, 201);
        assert!(response.body.contains("created"));
        assert!(!response.truncated);
        assert!(response.duration_ms >= 0);
        assert!(response
            .headers
            .iter()
            .any(|h| h.name == "content-type" && h.value == "application/json"));

        let raw = server.await.unwrap();
        assert!(raw.starts_with("POST /things"));
        assert!(raw.contains("x-devos-test: 1"));
        assert!(raw.contains("{\"name\":\"widget\"}"));
    }

    #[tokio::test]
    async fn rejects_invalid_method_and_url() {
        // Note: custom token methods like PROPFIND or TELEPORT! are legal
        // HTTP and deliberately allowed — only malformed tokens fail.
        let bad_method = ApiRequestSpec {
            method: "GE T".into(),
            url: "http://localhost".into(),
            headers: vec![],
            body: None,
        };
        assert!(matches!(
            send_request(&bad_method).await,
            Err(ApiError::Invalid(_))
        ));

        let bad_url = ApiRequestSpec {
            method: "GET".into(),
            url: "not a url".into(),
            headers: vec![],
            body: None,
        };
        assert!(matches!(
            send_request(&bad_url).await,
            Err(ApiError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn connection_refused_is_a_request_error() {
        let spec = ApiRequestSpec {
            method: "GET".into(),
            // Port 1 is essentially guaranteed closed.
            url: "http://127.0.0.1:1/".into(),
            headers: vec![],
            body: None,
        };
        assert!(matches!(
            send_request(&spec).await,
            Err(ApiError::Request(_))
        ));
    }
}
