use grok_search_rs::adapters::chat_completions_request::to_chat_completions_payload;
use grok_search_rs::model::search::{ContentBlock, SearchMessage, SearchRequest, SearchTool};
use serde_json::json;

fn sample_request() -> SearchRequest {
    SearchRequest {
        model: "grok-4.3-fast".into(),
        system: None,
        messages: vec![SearchMessage {
            role: "user".into(),
            content: vec![ContentBlock::text("hello")],
        }],
        tools: vec![SearchTool::web_search()],
    }
}

#[test]
fn payload_includes_tools_when_web_search_enabled() {
    let payload = to_chat_completions_payload(&sample_request(), "grok-4.3-fast", true);
    assert_eq!(payload["model"], "grok-4.3-fast");
    assert_eq!(payload["stream"], false);
    assert_eq!(payload["tools"], json!([{ "type": "web_search" }]));
    assert_eq!(payload["messages"][0]["role"], "system");
    assert_eq!(payload["messages"][1]["role"], "user");
    assert_eq!(payload["messages"][1]["content"], "hello");
}

#[test]
fn payload_omits_tools_when_disabled() {
    let payload = to_chat_completions_payload(&sample_request(), "grok-4.3-fast", false);
    assert!(
        payload.get("tools").is_none(),
        "tools must be absent when disabled"
    );
}

#[test]
fn user_system_overrides_default_hint() {
    let mut req = sample_request();
    req.system = Some("You are a cat.".into());
    let payload = to_chat_completions_payload(&req, "grok-4.3-fast", true);
    assert_eq!(payload["messages"][0]["content"], "You are a cat.");
}
