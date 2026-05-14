use crate::error::{GrokSearchError, Result};
use crate::model::source::Source;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct FirecrawlProvider {
    client: Client,
    api_url: String,
    api_key: String,
}

impl FirecrawlProvider {
    pub fn new(api_url: impl Into<String>, api_key: impl Into<String>, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            api_url: api_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    pub async fn search(&self, query: &str, max_results: usize) -> Result<Vec<Source>> {
        let raw = self
            .post_json("search", json!({ "query": query, "limit": max_results }))
            .await?;
        Ok(normalize_firecrawl_results(&raw))
    }

    pub async fn scrape(&self, url: &str) -> Result<String> {
        let raw = self
            .post_json("scrape", json!({ "url": url, "formats": ["markdown"] }))
            .await?;
        let content = raw
            .get("data")
            .and_then(|data| data.get("markdown").or_else(|| data.get("content")))
            .or_else(|| raw.get("markdown"))
            .or_else(|| raw.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|text| !text.trim().is_empty());

        content.ok_or_else(|| {
            GrokSearchError::Provider("Firecrawl scrape returned empty content".to_string())
        })
    }

    async fn post_json(&self, path: &str, body: Value) -> Result<Value> {
        let endpoint = format!("{}/{}", self.api_url, path.trim_start_matches('/'));
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| GrokSearchError::Provider(format!("Firecrawl request failed: {err}")))?;

        let status = response.status();
        let text = response.text().await.map_err(|err| {
            GrokSearchError::Provider(format!("Firecrawl body read failed: {err}"))
        })?;

        if !status.is_success() {
            return Err(GrokSearchError::Provider(format!(
                "Firecrawl returned HTTP {status}: {text}"
            )));
        }

        serde_json::from_str(&text)
            .map_err(|err| GrokSearchError::Parse(format!("invalid Firecrawl JSON: {err}")))
    }
}

pub fn normalize_firecrawl_results(raw: &Value) -> Vec<Source> {
    raw.get("data")
        .or_else(|| raw.get("results"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if let Some(url) = item.as_str() {
                return Some(Source::new(url, "firecrawl"));
            }
            let url = item.get("url").and_then(Value::as_str)?;
            let mut source = Source::new(url, "firecrawl");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(description) = item
                .get("description")
                .or_else(|| item.get("markdown"))
                .or_else(|| item.get("content"))
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
