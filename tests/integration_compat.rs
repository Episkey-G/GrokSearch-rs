//! End-to-end probe for the OpenAI-compatible transport. Skipped by default
//! because it hits live HTTP — run with:
//!
//!   GROK_SEARCH_E2E_CHAT_URL=https://your-gateway/v1\
//!   GROK_SEARCH_E2E_CHAT_KEY=sk-... \
//!   GROK_SEARCH_E2E_CHAT_MODEL=grok-4.3-fast \
//!   cargo test --test integration_compat -- --ignored --nocapture
//!
//! The probe asserts a non-empty response and at least one source. Use it as a
//! contract regression check when swapping gateways or upgrading providers.

use grok_search_rs::model::search::{ContentBlock, SearchMessage, SearchRequest, SearchTool};
use grok_search_rs::providers::openai_compatible::OpenAICompatProvider;
use std::time::Duration;

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn chat_completions_e2e_returns_text_and_sources() {
    let url = std::env::var("GROK_SEARCH_E2E_CHAT_URL")
        .expect("set GROK_SEARCH_E2E_CHAT_URL to run this test");
    let key = std::env::var("GROK_SEARCH_E2E_CHAT_KEY")
        .expect("set GROK_SEARCH_E2E_CHAT_KEY to run this test");
    let model = std::env::var("GROK_SEARCH_E2E_CHAT_MODEL")
        .unwrap_or_else(|_| "grok-4.3-fast".to_string());

    let provider = OpenAICompatProvider::new(url, key, &model, true, Duration::from_secs(60));
    let req = SearchRequest {
        model: model.clone(),
        system: None,
        messages: vec![SearchMessage {
            role: "user".into(),
            content: vec![ContentBlock::text(
                "What date did xAI release Grok 4.1 Fast? Answer briefly with a citation.",
            )],
        }],
        tools: vec![SearchTool::web_search()],
    };

    let resp = provider.search(&req).await.expect("e2e search");
    assert!(!resp.content.trim().is_empty(), "content was empty");
    assert!(
        !resp.sources.is_empty(),
        "expected at least one source, got 0; content was: {}",
        resp.content
    );
    eprintln!("content: {}", resp.content);
    eprintln!("sources: {} entries", resp.sources.len());
}
