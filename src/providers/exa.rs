//! Exa semantic search provider.
//!
//! Exa is an embeddings-first engine: strong on descriptive queries, papers,
//! official domains, and low-noise discovery, with native support for the
//! whole `SearchFilters` contract (domain include/exclude lists and a
//! published-date lower bound). Fetch goes through `/contents`. Auth accepts
//! `Authorization: Bearer`, so the shared bearer helpers apply as-is.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde_json::{json, Value};

use crate::error::{GrokSearchError, Result};
use crate::model::search::SearchFilters;
use crate::model::source::{FetchedPage, Source};

use super::http::{build_client, post_json};

/// Exa's public `numResults` ceiling.
const MAX_NUM_RESULTS: usize = 100;

#[derive(Clone)]
pub struct ExaProvider {
    client: Client,
    api_url: String,
    api_key: String,
}

impl ExaProvider {
    pub fn new(api_url: impl Into<String>, api_key: impl Into<String>, timeout: Duration) -> Self {
        Self::with_client(build_client(timeout), api_url, api_key)
    }

    /// Construct with an externally provided `reqwest::Client`. Used by
    /// `SearchService` to share one tuned client across providers.
    pub fn with_client(
        client: Client,
        api_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api_url: api_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.api_url, path.trim_start_matches('/'))
    }

    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        let body = exa_search_request_body(query, max_results, filters, now_unix_seconds());
        let raw = post_json(
            &self.client,
            &self.endpoint("search"),
            &self.api_key,
            &body,
            "Exa",
        )
        .await?;
        Ok(normalize_exa_results(&raw))
    }

    pub async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        let raw = post_json(
            &self.client,
            &self.endpoint("contents"),
            &self.api_key,
            &json!({ "urls": [url], "text": true }),
            "Exa",
        )
        .await?;
        parse_exa_contents(&raw, url)
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Search body without contents options: results carry metadata only
/// (title/url/publishedDate), keeping per-call cost at the base search rate —
/// inline page text comes from the shared enrichment pipeline, not from Exa.
pub fn exa_search_request_body(
    query: &str,
    max_results: usize,
    filters: &SearchFilters,
    now_unix: u64,
) -> Value {
    let mut body = json!({
        "query": query,
        "numResults": max_results.clamp(1, MAX_NUM_RESULTS),
    });
    let object = body.as_object_mut().expect("literal object");
    if !filters.include_domains.is_empty() {
        object.insert("includeDomains".into(), json!(filters.include_domains));
    }
    if !filters.exclude_domains.is_empty() {
        object.insert("excludeDomains".into(), json!(filters.exclude_domains));
    }
    if let Some(days) = filters.recency_days {
        object.insert(
            "startPublishedDate".into(),
            json!(start_published_date(days, now_unix)),
        );
    }
    body
}

/// `recency_days` → ISO-8601 lower bound for Exa's `startPublishedDate`:
/// midnight UTC `days` ago.
pub fn start_published_date(days: u32, now_unix: u64) -> String {
    let day_index = (now_unix / 86_400) as i64 - i64::from(days);
    let (year, month, day) = civil_from_days(day_index);
    format!("{year:04}-{month:02}-{day:02}T00:00:00.000Z")
}

/// Days-since-epoch → proleptic-Gregorian (year, month, day), after Howard
/// Hinnant's `civil_from_days`. One date subtraction does not justify a
/// calendar dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

pub fn normalize_exa_results(raw: &Value) -> Vec<Source> {
    raw.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?;
            let mut source = Source::new(url, "exa");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(summary) = item.get("summary").and_then(Value::as_str) {
                source = source.with_description(summary);
            }
            if let Some(date) = item.get("publishedDate").and_then(Value::as_str) {
                source = source.with_published_date(date);
            }
            Some(source)
        })
        .collect()
}

/// `/contents` reports per-URL failures in `statuses[]` beside a 200
/// response; an empty result set is inspected for that reason before the
/// generic "no content" verdict.
pub fn parse_exa_contents(raw: &Value, url: &str) -> Result<FetchedPage> {
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
                published_date: result
                    .get("publishedDate")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }
    let detail = raw
        .get("statuses")
        .and_then(Value::as_array)
        .and_then(|statuses| statuses.first())
        .and_then(|status| {
            status
                .get("error")
                .and_then(|error| error.get("tag"))
                .or_else(|| status.get("status"))
        })
        .and_then(Value::as_str)
        .unwrap_or("no content returned");
    Err(GrokSearchError::Provider(format!(
        "Exa contents failed for {url}: {detail}"
    )))
}
