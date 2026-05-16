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

use grok_search_rs::adapters::chat_completions_response::parse_chat_completions;

#[test]
fn extracts_openai_style_annotations() {
    let raw = json!({
        "choices": [{
            "message": {
                "content": "Result text.",
                "annotations": [
                    { "type": "url_citation", "url": "https://a.example/1", "title": "A" },
                    { "type": "url_citation", "url": "https://b.example/2" }
                ]
            }
        }]
    });
    let resp = parse_chat_completions(&raw).expect("parse");
    assert_eq!(resp.content, "Result text.");
    assert_eq!(resp.sources.len(), 2);
    assert_eq!(resp.sources[0].url, "https://a.example/1");
    assert_eq!(resp.sources[0].title.as_deref(), Some("A"));
}

#[test]
fn extracts_perplexity_style_message_citations() {
    let raw = json!({
        "choices": [{
            "message": {
                "content": "Body.",
                "citations": ["https://x.example", { "url": "https://y.example", "title": "Y" }]
            }
        }]
    });
    let resp = parse_chat_completions(&raw).expect("parse");
    assert_eq!(resp.sources.len(), 2);
    assert_eq!(resp.sources[1].title.as_deref(), Some("Y"));
}

#[test]
fn extracts_top_level_search_sources() {
    let raw = json!({
        "choices": [{ "message": { "content": "ok." } }],
        "search_sources": [
            { "url": "https://m.example/a", "title": "MA", "type": "web" },
            { "url": "https://m.example/b", "title": "MB", "type": "web" }
        ]
    });
    let resp = parse_chat_completions(&raw).expect("parse");
    assert_eq!(resp.sources.len(), 2);
    assert_eq!(resp.sources[0].url, "https://m.example/a");
}

#[test]
fn extracts_inline_bracket_citations_from_content() {
    let raw = json!({
        "choices": [{
            "message": { "content": "fact[[1]](https://c.example/p1) and[[2]](https://c.example/p2)." }
        }]
    });
    let resp = parse_chat_completions(&raw).expect("parse");
    let urls: Vec<_> = resp.sources.iter().map(|s| s.url.as_str()).collect();
    assert!(urls.contains(&"https://c.example/p1"));
    assert!(urls.contains(&"https://c.example/p2"));
}

#[test]
fn merges_and_dedupes_across_all_paths() {
    let raw = json!({
        "choices": [{
            "message": {
                "content": "see[[1]](https://dup.example/x) more.",
                "annotations": [
                    { "type": "url_citation", "url": "https://dup.example/x", "title": "Dup" }
                ],
                "citations": ["https://uniq.example/m"]
            }
        }],
        "search_sources": [{ "url": "https://uniq.example/n", "title": "N" }]
    });
    let resp = parse_chat_completions(&raw).expect("parse");
    let urls: Vec<_> = resp.sources.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(urls.len(), 3, "got {urls:?}");
    assert!(urls.contains(&"https://dup.example/x"));
    assert!(urls.contains(&"https://uniq.example/m"));
    assert!(urls.contains(&"https://uniq.example/n"));
}

#[test]
fn errors_when_content_empty_and_no_sources() {
    let raw = json!({ "choices": [{ "message": { "content": "" } }] });
    assert!(parse_chat_completions(&raw).is_err());
}
