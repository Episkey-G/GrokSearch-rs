use crate::adapters::grok_responses_request::to_grok_responses_payload;
use crate::adapters::grok_responses_response::parse_grok_responses;
use crate::error::Result;
use crate::model::search::{SearchRequest, SearchResponse};
use crate::providers::http::{build_client, post_json};
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
        Self {
            client: build_client(timeout),
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
        let raw = post_json(
            &self.client,
            &self.endpoint(),
            &self.api_key,
            &payload,
            "Grok Responses",
        )
        .await?;
        parse_grok_responses(&raw)
    }
}
