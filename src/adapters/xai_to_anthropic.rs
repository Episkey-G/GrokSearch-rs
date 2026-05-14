use crate::error::{GrokSearchError, Result};
use crate::model::anthropic::AnthropicResponse;
use crate::model::source::Source;
use serde_json::Value;

pub fn parse_xai_response(raw: &Value) -> Result<AnthropicResponse> {
    let content = output_text(raw).unwrap_or_default().trim().to_string();
    if content.is_empty() {
        return Err(GrokSearchError::Provider(
            "xAI response content is empty".to_string(),
        ));
    }

    let sources = raw
        .get("citations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(source_from_citation)
        .collect();

    Ok(AnthropicResponse { content, sources })
}

fn output_text(raw: &Value) -> Option<String> {
    if let Some(text) = raw.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    let output = raw.get("output")?.as_array()?;
    let mut parts = Vec::new();
    for item in output {
        let content = item.get("content").and_then(Value::as_array);
        if let Some(content) = content {
            for block in content {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
        }
    }
    Some(parts.join("\n"))
}

fn source_from_citation(value: &Value) -> Option<Source> {
    let url = value.get("url").and_then(Value::as_str)?.to_string();
    let mut source = Source::new(url, "xai_web_search");
    if let Some(title) = value.get("title").and_then(Value::as_str) {
        source = source.with_title(title);
    }
    if let Some(description) = value
        .get("description")
        .or_else(|| value.get("snippet"))
        .and_then(Value::as_str)
    {
        source = source.with_description(description);
    }
    Some(source)
}
