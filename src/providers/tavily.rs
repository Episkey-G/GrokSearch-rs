use crate::error::Result;
use crate::model::search::SearchFilters;
use crate::model::source::Source;
use crate::providers::http::{build_client, post_json};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

use crate::error::GrokSearchError;

#[derive(Clone)]
pub struct TavilyProvider {
    client: Client,
    api_url: String,
    api_key: String,
}

impl TavilyProvider {
    pub fn new(api_url: impl Into<String>, api_key: impl Into<String>, timeout: Duration) -> Self {
        Self::with_client(build_client(timeout), api_url, api_key)
    }

    /// Construct with an externally provided `reqwest::Client`. Used by
    /// `SearchService::new` to share one tuned client across providers.
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

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let endpoint = format!("{}/{}", self.api_url, path.trim_start_matches('/'));
        post_json(&self.client, &endpoint, &self.api_key, body, "Tavily").await
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
