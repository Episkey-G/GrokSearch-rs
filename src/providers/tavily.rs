use crate::error::Result;
use crate::model::search::SearchFilters;
use crate::model::source::{FetchedPage, Source};
use crate::providers::http::{build_client, post_json_with_status};
use crate::providers::keyring::{is_key_scoped_status, KeyRing};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

use crate::error::GrokSearchError;

#[derive(Clone)]
pub struct TavilyProvider {
    client: Client,
    api_url: String,
    keys: Arc<KeyRing>,
}

impl TavilyProvider {
    pub fn new(api_url: impl Into<String>, api_key: impl Into<String>, timeout: Duration) -> Self {
        Self::with_client(build_client(timeout), api_url, api_key)
    }

    /// Construct with an externally provided `reqwest::Client`. Used by
    /// `SearchService::new` to share one tuned client across providers.
    ///
    /// `api_key` accepts a single key or a comma-separated list; multiple
    /// keys are used round-robin with automatic failover on key-scoped
    /// errors (401/403/429/432/433).
    pub fn with_client(
        client: Client,
        api_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api_url: api_url.into().trim_end_matches('/').to_string(),
            keys: Arc::new(KeyRing::parse(&api_key.into())),
        }
    }

    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        let raw = self
            .post(
                "search",
                &tavily_search_request_body(query, max_results, filters),
            )
            .await?;
        Ok(normalize_tavily_results(&raw))
    }

    pub async fn extract(&self, url: &str) -> Result<FetchedPage> {
        let raw = self
            .post("extract", &json!({ "urls": [url], "format": "markdown" }))
            .await?;
        parse_tavily_extract(&raw)
    }

    pub async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        let raw = self
            .post("map", &tavily_map_request_body(url, max_results))
            .await?;
        Ok(limit_tavily_results(
            normalize_tavily_results(&raw),
            max_results,
        ))
    }

    /// POST with round-robin key selection. On a key-scoped failure
    /// (401/403/429/432/433) the request is retried once per remaining key;
    /// any other failure — timeout, 5xx, parse — returns immediately.
    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let endpoint = format!("{}/{}", self.api_url, path.trim_start_matches('/'));
        let attempts = self.keys.len();
        let start = self.keys.start();
        let mut last_error = None;
        for offset in 0..attempts {
            let key = self.keys.key(start + offset);
            match post_json_with_status(&self.client, &endpoint, key, body, "Tavily").await {
                Ok(value) => return Ok(value),
                Err(failure) => {
                    let key_scoped = failure.status.is_some_and(is_key_scoped_status);
                    if key_scoped && offset + 1 < attempts {
                        eprintln!(
                            "grok-search-rs: Tavily key {}/{} hit HTTP {}; rotating to next key",
                            (start + offset) % attempts + 1,
                            attempts,
                            failure.status.unwrap_or_default(),
                        );
                        last_error = Some(failure.error);
                        continue;
                    }
                    return Err(failure.error);
                }
            }
        }
        // Unreachable: the loop always returns on the final attempt. Kept as
        // a defensive fallback instead of unwrap/panic.
        Err(last_error.unwrap_or_else(|| {
            GrokSearchError::Provider("Tavily request failed with no attempts".to_string())
        }))
    }
}

pub fn tavily_search_request_body(
    query: &str,
    max_results: usize,
    filters: &SearchFilters,
) -> Value {
    #[derive(serde::Serialize)]
    struct TavilySearchBody<'a> {
        query: &'a str,
        max_results: usize,
        include_answer: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        days: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        topic: Option<&'static str>,
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        include_domains: &'a [String],
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        exclude_domains: &'a [String],
    }

    let body = TavilySearchBody {
        query,
        max_results,
        include_answer: false,
        days: filters.recency_days,
        topic: filters.recency_days.map(|_| "news"),
        include_domains: filters.include_domains.as_slice(),
        exclude_domains: filters.exclude_domains.as_slice(),
    };

    serde_json::to_value(&body).expect("tavily search body must serialize")
}

pub fn tavily_map_request_body(url: &str, max_results: usize) -> Value {
    json!({
        "url": url,
        "max_depth": 1,
        "limit": max_results
    })
}

pub fn limit_tavily_results(mut sources: Vec<Source>, max_results: usize) -> Vec<Source> {
    sources.truncate(max_results);
    sources
}

/// Parse a Tavily extract response into content + metadata. The extract
/// endpoint returns `title` alongside `raw_content` (verified live; the docs'
/// sample response omits it) but no published-date field, so `published_date`
/// is always `None` here.
pub fn parse_tavily_extract(raw: &Value) -> Result<FetchedPage> {
    let result = raw
        .get("results")
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let content = result
        .and_then(|item| item.get("raw_content").or_else(|| item.get("content")))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.trim().is_empty());

    let Some(content) = content else {
        return Err(GrokSearchError::Provider(
            "Tavily extract returned empty content".to_string(),
        ));
    };
    let title = result
        .and_then(|item| item.get("title"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.trim().is_empty());
    Ok(FetchedPage {
        content,
        title,
        published_date: None,
    })
}

/// Search results whose Tavily relevance `score` falls below this are junk,
/// not evidence. Long natural-language queries can drift Tavily onto one
/// generic word — observed live with "latest rmcp Rust MCP SDK release
/// version and what changed": dictionary and news-portal pages for "latest"
/// all scored ≤ 0.04, while on-topic results for answerable queries score
/// ≥ 0.49. 0.1 clears the junk band ~2.5x with ~5x headroom below on-topic
/// results. Items with no score (map results, API drift) always pass.
const MIN_SEARCH_SCORE: f64 = 0.1;

/// Normalize a Tavily `search`/`map` response into `Source`s. Search items
/// scoring below [`MIN_SEARCH_SCORE`] are dropped so keyword-drift junk never
/// reaches the enrichment/fallback source lists; score-less items (the map
/// endpoint returns bare URL strings) are kept unconditionally.
pub fn normalize_tavily_results(raw: &Value) -> Vec<Source> {
    raw.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if let Some(url) = item.as_str() {
                return Some(Source::new(url, "tavily"));
            }
            let url = item.get("url").and_then(Value::as_str)?;
            if item
                .get("score")
                .and_then(Value::as_f64)
                .is_some_and(|score| score < MIN_SEARCH_SCORE)
            {
                return None;
            }
            let mut source = Source::new(url, "tavily");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(description) = item
                .get("content")
                .or_else(|| item.get("description"))
                .and_then(Value::as_str)
            {
                source = source.with_description(description);
            }
            if let Some(published_date) = item.get("published_date").and_then(Value::as_str) {
                source = source.with_published_date(published_date);
            }
            Some(source)
        })
        .collect()
}

#[cfg(test)]
mod key_ring_tests {
    use super::*;

    #[test]
    fn rotation_cursor_is_shared_across_provider_clones() {
        let provider =
            TavilyProvider::with_client(Client::new(), "https://api.tavily.com", "tvly-a,tvly-b");
        let clone = provider.clone();
        // The cursor is shared (Arc): the clone continues the sequence rather
        // than restarting, regardless of the randomized starting offset.
        let a = provider.keys.start();
        assert_eq!(clone.keys.start(), (a + 1) % 2);
        assert_eq!(provider.keys.start(), (a + 2) % 2);
    }
}
