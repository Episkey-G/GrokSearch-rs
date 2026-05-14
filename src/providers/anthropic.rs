use crate::adapters::anthropic_messages::{parse_anthropic_sse, to_anthropic_messages_payload};
use crate::error::{GrokSearchError, Result};
use crate::model::anthropic::{AnthropicRequest, AnthropicResponse};
use reqwest::Client;

#[derive(Clone)]
pub struct AnthropicMessagesProvider {
    client: Client,
    api_url: String,
    api_key: String,
    require_web_search: bool,
    max_tokens: usize,
}

impl AnthropicMessagesProvider {
    pub fn new(
        api_url: impl Into<String>,
        api_key: impl Into<String>,
        require_web_search: bool,
        max_tokens: usize,
    ) -> Self {
        Self {
            client: Client::new(),
            api_url: api_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            require_web_search,
            max_tokens,
        }
    }

    pub fn endpoint(&self) -> String {
        format!("{}/messages", self.api_url)
    }

    pub async fn search(&self, request: &AnthropicRequest) -> Result<AnthropicResponse> {
        let payload =
            to_anthropic_messages_payload(request, self.require_web_search, self.max_tokens)?;
        let response = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await
            .map_err(|err| GrokSearchError::Provider(format!("Anthropic request failed: {err}")))?;

        let status = response.status();
        let body = response.text().await.map_err(|err| {
            GrokSearchError::Provider(format!("Anthropic body read failed: {err}"))
        })?;

        if !status.is_success() {
            return Err(GrokSearchError::Provider(format!(
                "Anthropic returned HTTP {status}: {body}"
            )));
        }

        if body.trim_start().starts_with('{') {
            let raw: serde_json::Value = serde_json::from_str(&body)
                .map_err(|err| GrokSearchError::Parse(format!("invalid Anthropic JSON: {err}")))?;
            return parse_anthropic_json(&raw);
        }

        parse_anthropic_sse(&body)
    }
}

fn parse_anthropic_json(raw: &serde_json::Value) -> Result<AnthropicResponse> {
    let mut content = String::new();
    if let Some(blocks) = raw.get("content").and_then(serde_json::Value::as_array) {
        for block in blocks {
            if block.get("type").and_then(serde_json::Value::as_str) == Some("text") {
                if let Some(text) = block.get("text").and_then(serde_json::Value::as_str) {
                    content.push_str(text);
                }
            }
        }
    }

    if content.trim().is_empty() {
        return Err(GrokSearchError::Provider(
            "Anthropic response content is empty".to_string(),
        ));
    }

    Ok(AnthropicResponse {
        content,
        sources: Vec::new(),
    })
}
