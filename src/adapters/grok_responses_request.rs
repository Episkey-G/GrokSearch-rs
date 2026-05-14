use crate::error::{GrokSearchError, Result};
use crate::model::search::{ContentBlock, SearchRequest};
use serde_json::{json, Value};

pub fn to_grok_responses_payload(
    req: &SearchRequest,
    require_web_search: bool,
    include_x_search: bool,
) -> Result<Value> {
    if require_web_search && !req.tools.iter().any(|tool| tool.name == "web_search") {
        return Err(GrokSearchError::Parse(
            "web_search is enabled but request does not include web_search tool intent".to_string(),
        ));
    }

    let mut input = Vec::new();
    if let Some(system) = req
        .system
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        input.push(json!({ "role": "system", "content": system }));
    }

    for message in &req.messages {
        let content = message
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        input.push(json!({ "role": message.role, "content": content }));
    }

    let mut tools = Vec::new();
    if require_web_search {
        tools.push(json!({ "type": "web_search" }));
    }
    if include_x_search {
        tools.push(json!({ "type": "x_search" }));
    }

    Ok(json!({
        "model": req.model,
        "input": input,
        "tools": tools
    }))
}
