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

// Live api.x.ai annotation objects have been observed carrying the citation
// index as the title ("1", "2" — issue #21). Those must be rejected at parse
// time so the field stays None and enrichment-time backfill can supply a real
// title; genuine titles must keep flowing through untouched.
#[test]
fn rejects_citation_index_titles_keeps_real_ones() {
    let raw = serde_json::json!({
        "output": [
            {
                "type": "web_search_call",
                "action": {
                    "sources": [
                        {"url": "https://example.com/a", "title": "1"},
                        {"url": "https://example.com/b", "title": "42"},
                        {"url": "https://example.com/c", "title": "x"},
                        {"url": "https://example.com/d", "title": " Real Page Title "}
                    ]
                }
            }
        ],
        "output_text": "answer"
    });

    let parsed = parse_grok_responses(&raw).expect("parsed");

    let title_of = |url: &str| {
        parsed
            .sources
            .iter()
            .find(|s| s.url == url)
            .expect("source present")
            .title
            .clone()
    };
    assert_eq!(title_of("https://example.com/a"), None, "numeric index");
    assert_eq!(title_of("https://example.com/b"), None, "multi-digit index");
    assert_eq!(title_of("https://example.com/c"), None, "single char");
    assert_eq!(
        title_of("https://example.com/d"),
        Some(" Real Page Title ".to_string()),
        "real titles must pass through unchanged"
    );
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

// Public-welfare / OpenAI-compatible Grok gateways proxy a real web search but
// serialize the citations as inline `[[n]](url)` Markdown in the answer text
// instead of the structured `annotations`/`citations`/`web_search_call` fields.
// Without inline extraction the structured-source list is empty, `web_search`
// misfires its source fallback, and every search degrades. The Responses parser
// must mirror the chat-completions adapter and harvest those inline links.
#[test]
fn parses_inline_bracket_citations_from_message_text() {
    let raw = serde_json::json!({
        "output": [
            {
                "type": "message",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Trump denied the guarantee.[[1]](https://www.nytimes.com/live/2026/06/07/us/trump-news) He was booed.[[2]](https://www.cnn.com/2026/06/08/us/trump-booed)"
                    }
                ]
            }
        ]
    });

    let parsed = parse_grok_responses(&raw).expect("parsed");

    let urls: Vec<_> = parsed.sources.iter().map(|s| s.url.as_str()).collect();
    assert!(
        urls.contains(&"https://www.nytimes.com/live/2026/06/07/us/trump-news"),
        "got {urls:?}"
    );
    assert!(
        urls.contains(&"https://www.cnn.com/2026/06/08/us/trump-booed"),
        "got {urls:?}"
    );
    assert!(parsed
        .sources
        .iter()
        .all(|source| source.provider == "grok_responses"));
}

#[test]
fn inline_citations_dedupe_against_structured_sources() {
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
                        "text": "See the news.[[1]](https://openai.com/news) Also new.[[2]](https://openai.com/blog)"
                    }
                ]
            }
        ]
    });

    let parsed = parse_grok_responses(&raw).expect("parsed");

    let urls: Vec<_> = parsed.sources.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(
        urls.len(),
        2,
        "structured + inline must union-dedupe, got {urls:?}"
    );
    assert!(urls.contains(&"https://openai.com/news"));
    assert!(urls.contains(&"https://openai.com/blog"));
}
