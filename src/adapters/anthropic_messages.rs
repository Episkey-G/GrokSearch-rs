use crate::error::{GrokSearchError, Result};
use crate::model::anthropic::{AnthropicRequest, AnthropicResponse};
use crate::model::source::Source;
use serde_json::{json, Value};

pub fn to_anthropic_messages_payload(
    req: &AnthropicRequest,
    require_web_search: bool,
    max_tokens: usize,
) -> Result<Value> {
    let has_web_search = req.tools.iter().any(|tool| tool.name == "web_search");
    if require_web_search && !has_web_search {
        return Err(GrokSearchError::Provider(
            "Anthropic Messages request requires web_search tool".to_string(),
        ));
    }

    let messages = req
        .messages
        .iter()
        .map(|message| {
            let text = message
                .content
                .iter()
                .map(|block| block.as_text())
                .collect::<Vec<_>>()
                .join("\n");
            json!({ "role": message.role, "content": text })
        })
        .collect::<Vec<_>>();

    let tools = if has_web_search {
        vec![json!({
            "type": "web_search_20250305",
            "name": "web_search",
            "max_uses": 5
        })]
    } else {
        Vec::new()
    };

    let mut payload = json!({
        "model": req.model,
        "max_tokens": max_tokens,
        "messages": messages,
        "tools": tools
    });

    if let Some(system) = &req.system {
        payload["system"] = json!(system);
    }

    Ok(payload)
}

pub fn parse_anthropic_sse(body: &str) -> Result<AnthropicResponse> {
    let mut content = String::new();
    let mut sources = Vec::new();

    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let value: Value = match serde_json::from_str(data) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if let Some(text) = value
            .get("delta")
            .and_then(|delta| delta.get("text"))
            .and_then(Value::as_str)
        {
            content.push_str(text);
        }

        if let Some(block) = value.get("content_block") {
            if block.get("type").and_then(Value::as_str) == Some("web_search_tool_result") {
                sources.extend(parse_search_result_sources(block));
            }
        }
    }

    if content.trim().is_empty() && !sources.is_empty() {
        content = format!("Search returned {} source(s).", sources.len());
    }

    if content.trim().is_empty() {
        return Err(GrokSearchError::Provider(
            "Anthropic response content is empty".to_string(),
        ));
    }

    Ok(AnthropicResponse { content, sources })
}

fn parse_search_result_sources(block: &Value) -> Vec<Source> {
    block
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if item.get("type").and_then(Value::as_str) != Some("web_search_result") {
                return None;
            }
            let url = item.get("url").and_then(Value::as_str)?;
            let mut source = Source::new(url, "anthropic_web_search");
            if let Some(title) = item.get("title").and_then(Value::as_str) {
                source = source.with_title(title);
            }
            if let Some(page_age) = item.get("page_age").and_then(Value::as_str) {
                source = source.with_published_date(page_age);
            }
            Some(source)
        })
        .collect()
}
