use async_trait::async_trait;
use grok_search_rs::error::Result;
use grok_search_rs::model::anthropic::{AnthropicRequest, AnthropicResponse};
use grok_search_rs::model::source::Source;
use grok_search_rs::model::tool::WebSearchInput;
use grok_search_rs::service::{AiProvider, SearchService, SourceProvider};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn web_search_returns_content_and_caches_sources() {
    let service = SearchService::fake_with_sources();

    let output = service
        .web_search(WebSearchInput {
            query: "2026年5月14日 OpenAI 最新新闻 官方公告".to_string(),
            platform: None,
            model: None,
            extra_sources: Some(2),
        })
        .await
        .expect("search output");

    assert!(!output.content.is_empty());
    assert!(output.sources_count > 0);
    assert_eq!(output.search_provider, "grok");
    assert!(!output.fallback_used);
    assert_eq!(output.fallback_reason, None);

    let sources = service
        .get_sources(&output.session_id)
        .await
        .expect("sources");
    assert_eq!(sources.sources_count, output.sources_count);
}

#[derive(Default)]
struct CountingSourceProvider {
    search_calls: Arc<Mutex<usize>>,
}

#[async_trait]
impl SourceProvider for CountingSourceProvider {
    async fn search_sources(&self, _query: &str, max_results: usize) -> Result<Vec<Source>> {
        *self.search_calls.lock().expect("search call lock") += 1;
        Ok((0..max_results)
            .map(|idx| Source::new(format!("https://example.com/enrichment-{idx}"), "tavily"))
            .collect())
    }

    async fn fetch(&self, _url: &str) -> Result<String> {
        Ok("fetched".to_string())
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

struct EmptySourcesAiProvider;

#[async_trait]
impl AiProvider for EmptySourcesAiProvider {
    async fn search(&self, _request: &AnthropicRequest) -> Result<AnthropicResponse> {
        Ok(AnthropicResponse {
            content: "This answer has no verifiable sources.".to_string(),
            sources: Vec::new(),
        })
    }
}

#[tokio::test]
async fn web_search_uses_env_default_extra_sources_after_grok_success() {
    let source_provider = CountingSourceProvider::default();
    let search_calls = source_provider.search_calls.clone();
    let service = SearchService::fake_with_custom_sources_and_config(
        Arc::new(source_provider),
        [("GROK_SEARCH_RS_EXTRA_SOURCES", "2")],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "OpenAI official updates".to_string(),
            platform: None,
            model: None,
            extra_sources: None,
        })
        .await
        .expect("search output");

    assert_eq!(output.search_provider, "grok");
    assert!(!output.fallback_used);
    assert_eq!(*search_calls.lock().expect("search call lock"), 1);
    assert_eq!(output.sources_count, 3);

    let cached = service
        .get_sources(&output.session_id)
        .await
        .expect("cached sources");

    assert!(cached
        .sources
        .iter()
        .any(|source| source.provider == "anthropic_web_search"));
    assert!(cached
        .sources
        .iter()
        .any(|source| source.provider == "tavily_enrichment"));
}

#[tokio::test]
async fn web_search_falls_back_to_tavily_when_grok_has_no_sources() {
    let source_provider = CountingSourceProvider::default();
    let search_calls = source_provider.search_calls.clone();
    let service = SearchService::fake_with_ai_and_sources(
        Arc::new(EmptySourcesAiProvider),
        Arc::new(source_provider),
        [("GROK_SEARCH_RS_FALLBACK_SOURCES", "4")],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "OpenAI official updates".to_string(),
            platform: None,
            model: None,
            extra_sources: None,
        })
        .await
        .expect("fallback output");

    assert_eq!(output.search_provider, "tavily_fallback");
    assert!(output.fallback_used);
    assert_eq!(
        output.fallback_reason,
        Some("grok_sources_empty".to_string())
    );
    assert_eq!(output.sources_count, 4);
    assert_eq!(*search_calls.lock().expect("search call lock"), 1);

    let cached = service
        .get_sources(&output.session_id)
        .await
        .expect("cached fallback sources");
    assert_eq!(cached.sources_count, 4);
    assert!(cached
        .sources
        .iter()
        .all(|source| source.provider == "tavily_fallback"));
}
