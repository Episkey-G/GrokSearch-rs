use async_trait::async_trait;
use grok_search_rs::error::Result;
use grok_search_rs::model::search::{SearchRequest, SearchResponse};
use grok_search_rs::model::source::Source;
use grok_search_rs::model::tool::WebSearchInput;
use grok_search_rs::service::{AiProvider, SearchService, SourceProvider};
use std::sync::{Arc, Mutex};

#[test]
fn service_requires_grok_search_api_key() {
    let cfg = grok_search_rs::config::Config::from_env_map([] as [(&str, &str); 0]);
    let result = SearchService::new(cfg);

    assert!(result.is_err());
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("GROK_SEARCH_API_KEY"));
}

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
    assert_eq!(output.search_provider, "grok_responses");
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
    async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
        Ok(SearchResponse {
            content: "This answer has no verifiable sources.".to_string(),
            sources: Vec::new(),
        })
    }
}

#[tokio::test]
async fn web_search_uses_env_default_extra_sources_after_grok_success() {
    let source_provider = CountingSourceProvider::default();
    let search_calls = source_provider.search_calls.clone();
    let service = SearchService::fake_custom(
        None,
        Arc::new(source_provider),
        None,
        [("GROK_SEARCH_EXTRA_SOURCES", "2")],
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

    assert_eq!(output.search_provider, "grok_responses");
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
        .any(|source| source.provider == "grok_responses"));
    assert!(cached
        .sources
        .iter()
        .any(|source| source.provider == "tavily_enrichment"));
}

#[tokio::test]
async fn web_search_falls_back_to_tavily_when_grok_has_no_sources() {
    let source_provider = CountingSourceProvider::default();
    let search_calls = source_provider.search_calls.clone();
    let service = SearchService::fake_custom(
        Some(Arc::new(EmptySourcesAiProvider)),
        Arc::new(source_provider),
        None,
        [("GROK_SEARCH_FALLBACK_SOURCES", "4")],
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

    assert_eq!(output.search_provider, "source_fallback");
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

struct FailingSourceProvider;

#[async_trait]
impl SourceProvider for FailingSourceProvider {
    async fn search_sources(&self, _query: &str, _max_results: usize) -> Result<Vec<Source>> {
        Err(grok_search_rs::error::GrokSearchError::Provider(
            "source failed".to_string(),
        ))
    }

    async fn fetch(&self, _url: &str) -> Result<String> {
        Err(grok_search_rs::error::GrokSearchError::Provider(
            "fetch failed".to_string(),
        ))
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Err(grok_search_rs::error::GrokSearchError::Provider(
            "map failed".to_string(),
        ))
    }
}

struct FirecrawlLikeSourceProvider;

#[async_trait]
impl SourceProvider for FirecrawlLikeSourceProvider {
    async fn search_sources(&self, _query: &str, max_results: usize) -> Result<Vec<Source>> {
        Ok((0..max_results)
            .map(|idx| Source::new(format!("https://firecrawl.example/{idx}"), "firecrawl"))
            .collect())
    }

    async fn fetch(&self, url: &str) -> Result<String> {
        Ok(format!("firecrawl fallback content for {url}"))
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn web_fetch_uses_firecrawl_when_tavily_fetch_fails() {
    let service = SearchService::fake_custom(
        None,
        Arc::new(FailingSourceProvider),
        Some(Arc::new(FirecrawlLikeSourceProvider)),
        [] as [(&str, &str); 0],
    );

    let content = service
        .web_fetch("https://example.com/article")
        .await
        .expect("fetch output");

    assert!(content.contains("firecrawl fallback content"));
}
