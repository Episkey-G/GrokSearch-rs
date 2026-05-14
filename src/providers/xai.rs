use crate::adapters::anthropic_to_xai::to_xai_responses_payload_with_x_search;
use crate::adapters::xai_to_anthropic::parse_xai_response;
use crate::error::{GrokSearchError, Result};
use crate::model::anthropic::{AnthropicRequest, AnthropicResponse};
use reqwest::Client;

#[derive(Clone)]
pub struct XaiResponsesProvider {
    client: Client,
    api_url: String,
    api_key: String,
    require_web_search: bool,
    include_x_search: bool,
}

impl XaiResponsesProvider {
    pub fn new(
        api_url: impl Into<String>,
        api_key: impl Into<String>,
        require_web_search: bool,
        include_x_search: bool,
    ) -> Self {
        Self {
            client: Client::new(),
            api_url: api_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            require_web_search,
            include_x_search,
        }
    }

    pub fn endpoint(&self) -> String {
        format!("{}/responses", self.api_url)
    }

    pub async fn search(&self, request: &AnthropicRequest) -> Result<AnthropicResponse> {
        let payload = to_xai_responses_payload_with_x_search(
            request,
            self.require_web_search,
            self.include_x_search,
        )?;
        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|err| GrokSearchError::Provider(format!("xAI request failed: {err}")))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| GrokSearchError::Provider(format!("xAI body read failed: {err}")))?;

        if !status.is_success() {
            return Err(GrokSearchError::Provider(format!(
                "xAI returned HTTP {status}: {body}"
            )));
        }

        let raw = serde_json::from_str(&body)
            .map_err(|err| GrokSearchError::Parse(format!("invalid xAI JSON: {err}")))?;
        parse_xai_response(&raw)
    }
}
