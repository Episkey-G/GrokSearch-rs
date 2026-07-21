//! End-to-end tests for Tavily multi-key rotation against a local mock HTTP
//! server. Uses a std::net listener on a background thread — no new dev
//! dependencies, no tokio `net` feature.

use grok_search_rs::model::search::SearchFilters;
use grok_search_rs::providers::tavily::TavilyProvider;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Minimal scripted HTTP/1.1 server: serves one canned response per expected
/// request, records each request's Authorization header, then exits.
/// Responses close the connection so reqwest never tries to reuse it.
fn spawn_mock_server(responses: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let base = format!("http://{}", listener.local_addr().expect("local addr"));
    let seen_auth = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&seen_auth);

    std::thread::spawn(move || {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set read timeout");

            // Read headers, then the Content-Length body so the client has
            // fully written the request before we respond.
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            let header_end = loop {
                let n = stream.read(&mut buf).expect("read request");
                if n == 0 {
                    break raw.len();
                }
                raw.extend_from_slice(&buf[..n]);
                if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let head = String::from_utf8_lossy(&raw[..header_end]).to_string();
            let content_length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            while raw.len() < header_end + content_length {
                let n = stream.read(&mut buf).expect("read body");
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
            }

            let auth = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("authorization")
                        .then(|| value.trim().to_string())
                })
                .unwrap_or_default();
            seen.lock().expect("auth log lock").push(auth);

            let reason = match status {
                200 => "OK",
                429 => "Too Many Requests",
                432 => "Plan Limit Exceeded",
                500 => "Internal Server Error",
                _ => "Mock",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        }
    });

    (base, seen_auth)
}

const OK_BODY: &str = r#"{"results":[{"url":"https://example.com","title":"ok"}]}"#;

fn provider_for(base: &str, keys: &str) -> TavilyProvider {
    TavilyProvider::new(base, keys, Duration::from_secs(5))
}

#[tokio::test]
async fn key_scoped_failure_rotates_to_next_key_and_succeeds() {
    let (base, auth_log) =
        spawn_mock_server(vec![(429, r#"{"error":"rate limited"}"#), (200, OK_BODY)]);
    let provider = provider_for(&base, "key-a,key-b");

    let sources = provider
        .search("rust", 3, &SearchFilters::default())
        .await
        .expect("second key should succeed after 429 on first");

    assert_eq!(sources.len(), 1);
    // The starting key is randomized per ring; the invariant is that the 429
    // triggers exactly one retry with the OTHER key, whichever came first.
    let log = auth_log.lock().expect("auth log lock").clone();
    assert_eq!(
        log.len(),
        2,
        "expected the 429 to trigger exactly one retry"
    );
    assert!(
        log.iter()
            .all(|auth| auth == "Bearer key-a" || auth == "Bearer key-b"),
        "unexpected auth headers: {log:?}"
    );
    assert_ne!(
        log[0], log[1],
        "expected the 429 to trigger a retry with the next key"
    );
}

#[tokio::test]
async fn plan_limit_432_rotates_like_rate_limit() {
    let (base, auth_log) =
        spawn_mock_server(vec![(432, r#"{"error":"plan limit"}"#), (200, OK_BODY)]);
    let provider = provider_for(&base, "key-a,key-b");

    provider
        .search("rust", 3, &SearchFilters::default())
        .await
        .expect("second key should succeed after 432 on first");

    assert_eq!(auth_log.lock().expect("auth log lock").len(), 2);
}

#[tokio::test]
async fn upstream_5xx_fails_fast_without_rotation() {
    let (base, auth_log) = spawn_mock_server(vec![(500, r#"{"error":"boom"}"#)]);
    let provider = provider_for(&base, "key-a,key-b");

    let err = provider
        .search("rust", 3, &SearchFilters::default())
        .await
        .expect_err("500 is upstream-wide and must not rotate");

    assert!(err.to_string().contains("500"), "got: {err}");
    assert_eq!(
        auth_log.lock().expect("auth log lock").len(),
        1,
        "5xx must consume exactly one attempt"
    );
}

#[tokio::test]
async fn successive_requests_round_robin_across_keys() {
    let (base, auth_log) = spawn_mock_server(vec![(200, OK_BODY), (200, OK_BODY)]);
    let provider = provider_for(&base, "key-a,key-b");

    for _ in 0..2 {
        provider
            .search("rust", 3, &SearchFilters::default())
            .await
            .expect("mock returns 200");
    }

    // The starting key is randomized per ring; round-robin means the second
    // request must consume the OTHER key, whichever the ring started on.
    let log = auth_log.lock().expect("auth log lock").clone();
    assert_eq!(log.len(), 2);
    assert!(
        log.iter()
            .all(|auth| auth == "Bearer key-a" || auth == "Bearer key-b"),
        "unexpected auth headers: {log:?}"
    );
    assert_ne!(
        log[0], log[1],
        "two successful requests should consume credits from different keys"
    );
}

#[tokio::test]
async fn single_key_exhausts_without_retry() {
    let (base, auth_log) = spawn_mock_server(vec![(429, r#"{"error":"rate limited"}"#)]);
    let provider = provider_for(&base, "only-key");

    provider
        .search("rust", 3, &SearchFilters::default())
        .await
        .expect_err("single key has nothing to rotate to");

    assert_eq!(auth_log.lock().expect("auth log lock").len(), 1);
}
