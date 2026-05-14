use grok_search_rs::adapters::grok_responses_request::to_grok_responses_payload;
use grok_search_rs::adapters::grok_responses_response::parse_grok_responses;
use grok_search_rs::model::search::{ContentBlock, SearchMessage, SearchRequest, SearchTool};

fn sample_request() -> SearchRequest {
    SearchRequest {
        model: "grok-4.3".to_string(),
        system: Some("Use web search.".to_string()),
        messages: vec![SearchMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::text("latest OpenAI announcement")],
        }],
        tools: vec![SearchTool::web_search()],
    }
}

#[test]
fn grok_responses_payload_includes_web_search_by_default() {
    let payload = to_grok_responses_payload(&sample_request(), true, false).expect("payload");

    assert_eq!(payload["model"], "grok-4.3");
    assert!(payload.get("messages").is_none());
    assert_eq!(payload["input"][0]["role"], "system");
    assert_eq!(payload["input"][1]["role"], "user");
    assert_eq!(payload["tools"][0]["type"], "web_search");
    assert_eq!(payload["tools"].as_array().unwrap().len(), 1);
}

#[test]
fn grok_responses_payload_adds_x_search_only_when_enabled() {
    let payload = to_grok_responses_payload(&sample_request(), true, true).expect("payload");

    assert_eq!(payload["tools"][0]["type"], "web_search");
    assert_eq!(payload["tools"][1]["type"], "x_search");
}

#[test]
fn web_search_enabled_requires_tool_intent() {
    let mut req = sample_request();
    req.tools = Vec::new();

    let err = to_grok_responses_payload(&req, true, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("web_search"));
}

#[test]
fn parses_grok_responses_text_annotations_and_search_call_sources() {
    let raw = serde_json::json!({
        "output": [
            {
                "type": "web_search_call",
                "action": {
                    "sources": [
                        {"url": "https://openai.com/news", "title": "OpenAI News"}
                    ]
                }
            },
            {
                "type": "message",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Here is the answer.",
                        "annotations": [
                            {"url": "https://platform.openai.com/docs", "title": "Docs"}
                        ]
                    }
                ]
            }
        ]
    });

    let parsed = parse_grok_responses(&raw).expect("parsed");

    assert_eq!(parsed.content, "Here is the answer.");
    assert_eq!(parsed.sources.len(), 2);
    assert!(parsed
        .sources
        .iter()
        .all(|source| source.provider == "grok_responses"));
    assert!(parsed
        .sources
        .iter()
        .any(|source| source.url == "https://openai.com/news"));
    assert!(parsed
        .sources
        .iter()
        .any(|source| source.url == "https://platform.openai.com/docs"));
}

#[test]
fn parses_output_text_fallback() {
    let raw = serde_json::json!({
        "output_text": "Compact answer",
        "citations": ["https://example.com/a"]
    });

    let parsed = parse_grok_responses(&raw).expect("parsed");

    assert_eq!(parsed.content, "Compact answer");
    assert_eq!(parsed.sources[0].url, "https://example.com/a");
}
