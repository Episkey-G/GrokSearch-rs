use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use crate::error::{GrokSearchError, Result};

/// Build a `reqwest::Client` with the configured timeout. Falls back to the
/// default client if the builder errors (e.g. invalid TLS config in tests).
pub fn build_client(timeout: Duration) -> Client {
    Client::builder()
        .timeout(timeout)
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
    let text = response
        .text()
        .await
        .map_err(|err| GrokSearchError::Provider(format!("{label} body read failed: {err}")))?;

    if !status.is_success() {
        return Err(GrokSearchError::Provider(format!(
            "{label} returned HTTP {status}: {text}"
        )));
    }

    serde_json::from_str(&text)
        .map_err(|err| GrokSearchError::Parse(format!("invalid {label} JSON: {err}")))
}
