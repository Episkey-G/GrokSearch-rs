use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use crate::error::{GrokSearchError, Result};

/// Build a tuned `reqwest::Client`. The same client is shared across providers
/// so TLS sessions and keep-alive connections can be reused between providers
/// that hit different hosts. Falls back to a bare `Client::new()` if the
/// builder errors (preserves prior behavior for tests that construct providers
/// without env-driven config).
pub fn build_client(timeout: Duration) -> Client {
    Client::builder()
        .timeout(timeout)
        .gzip(true)
        .pool_idle_timeout(Some(Duration::from_secs(90)))
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .tcp_nodelay(true)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Issue an authenticated JSON POST and normalize transport / status / parse
/// errors into `GrokSearchError`. `label` appears in error messages to
/// distinguish upstream providers (e.g. "Tavily", "Firecrawl", "Grok Responses").
pub async fn post_json(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    body: &Value,
    label: &str,
) -> Result<Value> {
    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(body)
        .send()
        .await
        .map_err(|err| {
            if err.is_timeout() {
                GrokSearchError::Timeout(format!("{label} request timed out: {err}"))
            } else {
                GrokSearchError::Provider(format!("{label} request failed: {err}"))
            }
        })?;

    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| GrokSearchError::Provider(format!("{label} body read failed: {err}")))?;

    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(GrokSearchError::Provider(format!(
            "{label} returned HTTP {status}: {text}"
        )));
    }

    serde_json::from_slice(&bytes)
        .map_err(|err| GrokSearchError::Parse(format!("invalid {label} JSON: {err}")))
}
