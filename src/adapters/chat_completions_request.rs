use crate::model::search::{ContentBlock, SearchRequest};
use serde_json::{json, Value};

const DEFAULT_SYSTEM_HINT: &str =
    "You may search the web when helpful. \
     When you cite a fact, append the source URL inline so the caller can extract it.";

/// Build an OpenAI-style `/v1/chat/completions` payload from a generic
/// `SearchRequest`. The user's `system` (if any) takes precedence over the
/// built-in hint; otherwise the hint nudges chat-style gateways that auto-search.
///
/// `include_web_search_tool` mirrors `Config.web_search_enabled`. Some gateways
/// (e.g. modelverse) honor `tools:[{"type":"web_search"}]`; others (e.g.
/// marybrown) ignore it harmlessly. We always send it when enabled — it is
/// either useful or a no-op, never a failure mode.
pub fn to_chat_completions_payload(
    req: &SearchRequest,
    model: &str,
    include_web_search_tool: bool,
) -> Value {
    let system_text = req
        .system
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(DEFAULT_SYSTEM_HINT);

    let mut messages = vec![json!({ "role": "system", "content": system_text })];
    for message in &req.messages {
        let content = message
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        messages.push(json!({ "role": message.role, "content": content }));
    }

    let mut payload = json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });
    if include_web_search_tool {
        payload["tools"] = json!([{ "type": "web_search" }]);
    }
    payload
}
