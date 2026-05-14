use crate::error::{GrokSearchError, Result};
use crate::model::anthropic::AnthropicRequest;
use serde_json::{json, Value};

pub fn to_xai_responses_payload(req: &AnthropicRequest, require_web_search: bool) -> Result<Value> {
    to_xai_responses_payload_with_x_search(req, require_web_search, true)
}

pub fn to_xai_responses_payload_with_x_search(
    req: &AnthropicRequest,
    require_web_search: bool,
    include_x_search: bool,
) -> Result<Value> {
    let mut input = Vec::new();
    if let Some(system) = &req.system {
        input.push(json!({ "role": "system", "content": system }));
    }

    for message in &req.messages {
        let text = message
            .content
            .iter()
            .map(|block| block.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        input.push(json!({ "role": message.role, "content": text }));
    }

    let has_web_search = req.tools.iter().any(|tool| tool.name == "web_search");
    if require_web_search && !has_web_search {
        return Err(GrokSearchError::Provider(
            "xAI Responses request requires web_search tool".to_string(),
        ));
    }

    let tools = if has_web_search {
        let mut tools = vec![json!({ "type": "web_search" })];
        if include_x_search {
            tools.push(json!({ "type": "x_search" }));
        }
        tools
    } else {
        Vec::new()
    };

    Ok(json!({
        "model": req.model,
        "input": input,
        "tools": tools
    }))
}
