use crate::adapters::grok_responses_request::to_grok_responses_payload;
use crate::adapters::grok_responses_response::parse_grok_responses;
use crate::error::{GrokSearchError, Result};
use crate::model::search::{SearchRequest, SearchResponse};
use reqwest::Client;
use std::time::Duration;

#[derive(Clone)]
pub struct GrokResponsesProvider {
    client: Client,
    api_url: String,
    api_key: String,
    require_web_search: bool,
    include_x_search: bool,
}

impl GrokResponsesProvider {
    pub fn new(
        api_url: impl Into<String>,
        api_key: impl Into<String>,
        require_web_search: bool,
        include_x_search: bool,
        timeout: Duration,
    ) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            api_url: api_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            require_web_search,
            include_x_search,
        }
    }

    pub fn endpoint(&self) -> String {
        format!("{}/responses", self.api_url)
    }

    pub async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
        let payload =
            to_grok_responses_payload(request, self.require_web_search, self.include_x_search)?;
        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|err| {
                GrokSearchError::Provider(format!("Grok Responses request failed: {err}"))
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|err| {
            GrokSearchError::Provider(format!("Grok Responses body read failed: {err}"))
        })?;

        if !status.is_success() {
            return Err(GrokSearchError::Provider(format!(
                "Grok Responses returned HTTP {status}: {body}"
            )));
        }

        let raw = serde_json::from_str(&body)
            .map_err(|err| GrokSearchError::Parse(format!("invalid Grok Responses JSON: {err}")))?;
        parse_grok_responses(&raw)
    }
}
