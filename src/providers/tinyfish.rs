//! TinyFish Search & Fetch provider (issue #12).
//!
//! Two independent endpoints share one API key: search is a GET service at
//! `api.search.tinyfish.ai`, fetch a POST service at `api.fetch.tinyfish.ai`.
//! Both authenticate with an `X-API-Key` header rather than a bearer token,
//! and neither consumes account credits (rate limits still apply).

use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};

use crate::error::{GrokSearchError, Result};
use crate::model::search::SearchFilters;
use crate::model::source::{FetchedPage, Source};

use super::http::{build_client, get_json_with_header_auth, post_json_with_header_auth};

const AUTH_HEADER: &str = "X-API-Key";
/// TinyFish caps `recency_minutes` at ten years.
const MAX_RECENCY_MINUTES: u64 = 5_256_000;

#[derive(Clone)]
pub struct TinyfishProvider {
    client: Client,
    search_api_url: String,
    fetch_api_url: String,
    api_key: String,
}

impl TinyfishProvider {
    pub fn new(
        search_api_url: impl Into<String>,
        fetch_api_url: impl Into<String>,
        api_key: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self::with_client(
            build_client(timeout),
            search_api_url,
            fetch_api_url,
            api_key,
        )
    }

    /// Construct with an externally provided `reqwest::Client`. Used by
    /// `SearchService` to share one tuned client across providers.
    pub fn with_client(
        client: Client,
        search_api_url: impl Into<String>,
        fetch_api_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            client,
            search_api_url: search_api_url.into().trim_end_matches('/').to_string(),
            fetch_api_url: fetch_api_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        let raw = get_json_with_header_auth(
            &self.client,
            &self.search_api_url,
            &tinyfish_search_query(query, filters),
            (AUTH_HEADER, &self.api_key),
            "TinyFish",
        )
        .await?;
        // The API has no result-count parameter (only pagination), so the
        // caller's budget is applied client-side.
        let mut sources = normalize_tinyfish_results(&raw);
        sources.truncate(max_results);
        Ok(sources)
    }

    pub async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let raw = post_json_with_header_auth(
            &self.client,
            &self.fetch_api_url,
            (AUTH_HEADER, &self.api_key),
            &json!({ "urls": [url], "format": "markdown" }),
            "TinyFish",
        )
        .await?;
        parse_tinyfish_fetch(&raw, url)
    }
}

/// Build the GET query parameters. TinyFish has no structured domain
/// parameters — domain scoping rides inside the query string as `site:` /
/// `-site:` search operators (the documented mechanism) — while recency maps
/// onto `recency_minutes`.
pub fn tinyfish_search_query(query: &str, filters: &SearchFilters) -> Vec<(&'static str, String)> {
    let mut composed = query.to_string();
    match filters.include_domains.as_slice() {
        [] => {}
        [single] => {
            composed.push_str(" site:");
            composed.push_str(single);
        }
        many => {
            let group = many
                .iter()
                .map(|domain| format!("site:{domain}"))
                .collect::<Vec<_>>()
                .join(" OR ");
            composed.push_str(&format!(" ({group})"));
        }
    }
    for domain in &filters.exclude_domains {
        composed.push_str(" -site:");
        composed.push_str(domain);
    }
    let mut params = vec![("query", composed)];
    if let Some(days) = filters.recency_days {
        let minutes = u64::from(days)
            .saturating_mul(24 * 60)
            .clamp(1, MAX_RECENCY_MINUTES);
        params.push(("recency_minutes", minutes.to_string()));
    }
    params
}

pub fn normalize_tinyfish_results(raw: &Value) -> Vec<Source> {
    raw.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?;
            let mut source = Source::new(url, "tinyfish");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(snippet) = item.get("snippet").and_then(Value::as_str) {
                source = source.with_description(snippet);
            }
            if let Some(date) = item.get("date").and_then(Value::as_str) {
                source = source.with_published_date(date);
            }
            Some(source)
        })
        .collect()
}

/// Per-URL failures (timeouts, anti-bot blocks) arrive in `errors[]` beside a
/// 200 response, so an empty `results` is inspected for a reason before the
/// generic "no content" verdict.
pub fn parse_tinyfish_fetch(raw: &Value, url: &str) -> Result<FetchedPage> {
    if let Some(result) = raw
        .get("results")
        .and_then(Value::as_array)
        .and_then(|results| results.first())
    {
        let content = result
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !content.trim().is_empty() {
            return Ok(FetchedPage {
                content,
                title: result
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                published_date: None,
            });
        }
    }
    let detail = raw
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| error.get("error").and_then(Value::as_str))
        .unwrap_or("no content returned");
    Err(GrokSearchError::Provider(format!(
        "TinyFish fetch failed for {url}: {detail}"
    )))
}
