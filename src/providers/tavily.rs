use crate::error::Result;
use crate::model::search::SearchFilters;
use crate::model::source::Source;
use crate::providers::http::{build_client, post_json_with_status};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::error::GrokSearchError;

/// Round-robin ring over one or more Tavily API keys. Shared (`Arc`) across
/// provider clones so the rotation cursor is global: each request starts on
/// the next key, spreading credit consumption evenly across all keys.
struct KeyRing {
    keys: Vec<String>,
    cursor: AtomicUsize,
}

impl KeyRing {
    /// Split a comma-separated key list into the ring. Whitespace around each
    /// segment is trimmed and empty segments are dropped, so `"a, b,"` yields
    /// two keys. When no non-empty segment remains (e.g. the raw value is
    /// empty), the raw value is kept as a single key — preserving the previous
    /// single-key behavior where a bogus key simply fails upstream with 401.
    fn parse(raw: &str) -> Self {
        let mut keys: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect();
        if keys.is_empty() {
            keys.push(raw.to_string());
        }
        // Start at a random offset. On the HTTP transport the provider (and this
        // ring) is rebuilt per request, so a fixed start of 0 would make every
        // request begin on the first key and concentrate load there. A random
        // start spreads load across keys statelessly; stdio keeps its persistent
        // round-robin (the random start is just a one-time offset).
        let start = random_offset(keys.len());
        Self {
            keys,
            cursor: AtomicUsize::new(start),
        }
    }

    fn len(&self) -> usize {
        self.keys.len()
    }

    /// Index the next request should start from. `Relaxed` is enough: the
    /// cursor only needs even distribution, not cross-request ordering.
    fn start(&self) -> usize {
        self.cursor.fetch_add(1, Ordering::Relaxed) % self.keys.len()
    }

    fn key(&self, index: usize) -> &str {
        &self.keys[index % self.keys.len()]
    }
}

/// A best-effort random starting offset in `0..len` (falls back to 0 if the RNG
/// is unavailable). Only meaningful for multi-key rings.
fn random_offset(len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let mut buf = [0u8; 8];
    match getrandom::fill(&mut buf) {
        Ok(()) => (u64::from_ne_bytes(buf) % len as u64) as usize,
        Err(_) => 0,
    }
}

/// HTTP statuses that indict the *key* rather than the request or upstream:
/// 401/403 (invalid or unauthorized key), 429 (per-key rate limit),
/// 432 (Tavily plan limit exceeded), 433 (Tavily pay-as-you-go limit
/// exceeded). Only these trigger rotation to the next key — timeouts and
/// 5xx are upstream-wide, so retrying with another key would just add
/// latency.
fn is_key_scoped_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 429 | 432 | 433)
}

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

    pub async fn extract(&self, url: &str) -> Result<String> {
        let raw = self
            .post("extract", &json!({ "urls": [url], "format": "markdown" }))
            .await?;
        let extracted = raw
            .get("results")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("raw_content").or_else(|| item.get("content")))
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty());

        extracted.ok_or_else(|| {
            GrokSearchError::Provider("Tavily extract returned empty content".to_string())
        })
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
    fn single_key_parses_to_one_entry() {
        let ring = KeyRing::parse("tvly-only");
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.key(0), "tvly-only");
    }

    #[test]
    fn comma_separated_keys_split_trim_and_drop_empties() {
        let ring = KeyRing::parse(" tvly-a, tvly-b ,, tvly-c,");
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.key(0), "tvly-a");
        assert_eq!(ring.key(1), "tvly-b");
        assert_eq!(ring.key(2), "tvly-c");
    }

    #[test]
    fn all_empty_segments_fall_back_to_raw_value() {
        // Preserves the legacy single-key path: a degenerate value still
        // produces one key that fails upstream, instead of an empty ring.
        let ring = KeyRing::parse("");
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.key(0), "");
    }

    #[test]
    fn start_rotates_round_robin_across_requests() {
        // The starting key is randomized per ring, but consecutive starts still
        // advance by one and wrap — round-robin is preserved.
        let ring = KeyRing::parse("a,b,c");
        let first = ring.start();
        assert_eq!(ring.start(), (first + 1) % 3);
        assert_eq!(ring.start(), (first + 2) % 3);
        assert_eq!(ring.start(), (first + 3) % 3);
    }

    #[test]
    fn single_key_ring_always_starts_at_zero() {
        // A single-key ring has a deterministic (0) start — no randomization.
        let ring = KeyRing::parse("solo");
        assert_eq!(ring.start(), 0);
        assert_eq!(ring.start(), 0);
    }

    #[test]
    fn key_indexing_wraps_for_failover_offsets() {
        let ring = KeyRing::parse("a,b");
        assert_eq!(ring.key(2), "a");
        assert_eq!(ring.key(3), "b");
    }

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

    #[test]
    fn key_scoped_statuses_trigger_rotation_only() {
        for status in [401, 403, 429, 432, 433] {
            assert!(is_key_scoped_status(status), "expected rotate on {status}");
        }
        for status in [400, 404, 408, 500, 502, 503] {
            assert!(!is_key_scoped_status(status), "must not rotate on {status}");
        }
    }
}
