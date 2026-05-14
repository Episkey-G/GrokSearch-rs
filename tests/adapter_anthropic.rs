use grok_search_rs::adapters::anthropic_messages::{
    parse_anthropic_sse, to_anthropic_messages_payload,
};
use grok_search_rs::adapters::anthropic_to_xai::to_xai_responses_payload;
use grok_search_rs::model::anthropic::{
    AnthropicMessage, AnthropicRequest, AnthropicTool, ContentBlock,
};

#[test]
fn anthropic_payload_includes_server_web_search_tool() {
    let req = AnthropicRequest {
        model: "grok-4-1-fast-reasoning".to_string(),
        system: Some("Use web search.".to_string()),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::text("latest OpenAI announcement")],
        }],
        tools: vec![AnthropicTool::web_search()],
    };

    let payload = to_anthropic_messages_payload(&req, true, 1024).expect("payload");

    assert_eq!(payload["model"], "grok-4-1-fast-reasoning");
    assert_eq!(payload["max_tokens"], 1024);
    assert_eq!(payload["tools"][0]["type"], "web_search_20250305");
    assert_eq!(payload["tools"][0]["name"], "web_search");
}

#[test]
fn openai_responses_payload_includes_web_search_and_x_search_tools() {
    let req = AnthropicRequest {
        model: "grok-4.3".to_string(),
        system: Some("Use web search.".to_string()),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::text("latest OpenAI announcement")],
        }],
        tools: vec![AnthropicTool::web_search()],
    };

    let payload = to_xai_responses_payload(&req, true).expect("payload");

    assert_eq!(payload["model"], "grok-4.3");
    assert!(payload.get("messages").is_none());
    assert_eq!(payload["input"][0]["role"], "system");
    assert_eq!(payload["input"][1]["role"], "user");
    assert_eq!(payload["tools"][0]["type"], "web_search");
    assert_eq!(payload["tools"][1]["type"], "x_search");
}

#[test]
fn web_search_enabled_requires_anthropic_tool() {
    let req = AnthropicRequest {
        model: "grok-4-1-fast-reasoning".to_string(),
        system: None,
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::text("latest OpenAI news")],
        }],
        tools: vec![],
    };

    let err = to_anthropic_messages_payload(&req, true, 1024)
        .unwrap_err()
        .to_string();
    assert!(err.contains("web_search"));
}

#[test]
fn parses_anthropic_sse_text_and_web_search_results() {
    let sse = r#"event: content_block_delta
data: {"delta":{"text":"Here is the answer.","type":"text_delta"},"index":0,"type":"content_block_delta"}

event: content_block_start
data: {"content_block":{"content":[{"title":"OpenAI News","type":"web_search_result","url":"https://openai.com/news"}],"tool_use_id":"srvtoolu_1","type":"web_search_tool_result"},"index":1,"type":"content_block_start"}

event: message_stop
data: {"type":"message_stop"}
"#;

    let parsed = parse_anthropic_sse(sse).expect("parsed");
    assert_eq!(parsed.content, "Here is the answer.");
    assert_eq!(parsed.sources.len(), 1);
    assert_eq!(parsed.sources[0].provider, "anthropic_web_search");
    assert_eq!(parsed.sources[0].url, "https://openai.com/news");
}

#[test]
fn sse_with_only_search_results_returns_search_summary_content() {
    let sse = r#"event: content_block_start
data: {"content_block":{"content":[{"title":"OpenAI News","type":"web_search_result","url":"https://openai.com/news"}],"tool_use_id":"srvtoolu_1","type":"web_search_tool_result"},"index":1,"type":"content_block_start"}
"#;

    let parsed = parse_anthropic_sse(sse).expect("parsed");
    assert!(parsed.content.contains("Search returned 1 source"));
    assert_eq!(parsed.sources.len(), 1);
}
