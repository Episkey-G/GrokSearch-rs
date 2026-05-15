use crate::error::Result;
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
        Self {
            client: build_client(timeout),
            api_url: api_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    pub async fn search(&self, query: &str, max_results: usize) -> Result<Vec<Source>> {
        let raw = self
            .post(
                "search",
                &json!({
                    "query": query,
                    "max_results": max_results,
                    "include_answer": false
                }),
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
