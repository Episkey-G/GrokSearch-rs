//! End-to-end probe through the full `SearchService` for the OpenAI-compatible
//! transport. Skipped by default — run with:
//!
//!   GROK_SEARCH_E2E_CHAT_URL=https://api.modelverse.cn/v1 \
//!   GROK_SEARCH_E2E_CHAT_KEY=... \
//!   GROK_SEARCH_E2E_CHAT_MODEL=grok-4-1-fast-reasoning \
//!   cargo test --test integration_service -- --ignored --nocapture

use grok_search_rs::config::{Config, Transport};
use grok_search_rs::model::tool::WebSearchInput;
use grok_search_rs::service::SearchService;

#[tokio::test(flavor = "current_thread")]
#[ignore]
async fn full_service_web_search_via_chat_completions() {
    let url = std::env::var("GROK_SEARCH_E2E_CHAT_URL").expect("set GROK_SEARCH_E2E_CHAT_URL");
    let key = std::env::var("GROK_SEARCH_E2E_CHAT_KEY").expect("set GROK_SEARCH_E2E_CHAT_KEY");
    let model = std::env::var("GROK_SEARCH_E2E_CHAT_MODEL")
        .unwrap_or_else(|_| "grok-4.3-fast".to_string());

    let config = Config::from_env_map([
        ("OPENAI_COMPATIBLE_API_URL", url.as_str()),
        ("OPENAI_COMPATIBLE_API_KEY", key.as_str()),
        ("OPENAI_COMPATIBLE_MODEL", model.as_str()),
        ("TAVILY_ENABLED", "false"),
        ("FIRECRAWL_ENABLED", "false"),
    ]);
    assert_eq!(config.transport, Transport::ChatCompletions);

    let svc = SearchService::new(config).expect("service");
    let out = svc
        .web_search(WebSearchInput {
            query: "What date did xAI release Grok 4.1 Fast? Answer with a citation.".into(),
            platform: None,
            recency_days: None,
            include_domains: vec![],
            exclude_domains: vec![],
            extra_sources: None,
            model: None,
        })
        .await
        .expect("web_search");

    assert!(!out.content.trim().is_empty(), "content empty");
    eprintln!("content head: {}", &out.content.chars().take(200).collect::<String>());
    eprintln!("sources_count: {}", out.sources_count);
    eprintln!("session_id: {}", out.session_id);
}
