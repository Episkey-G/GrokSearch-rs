use async_trait::async_trait;
use grok_search_rs::error::Result;
use grok_search_rs::model::search::{SearchFilters, SearchRequest, SearchResponse};
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
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(!output.content.is_empty());
    assert!(output.sources_count > 0);
    assert_eq!(output.search_provider, "grok_responses");
    assert!(!output.fallback_used);
    assert_eq!(output.fallback_reason, None);
    assert_eq!(output.sources.len(), output.sources_count);
    assert!(output
        .sources
        .iter()
        .any(|source| source.provider == "grok_responses"));

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
    async fn search_sources(
        &self,
        _query: &str,
        max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
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
            ..Default::default()
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
            ..Default::default()
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

    assert_eq!(output.sources.len(), 4);
    assert!(output
        .sources
        .iter()
        .all(|item| item.provider == "tavily_fallback"));
}

#[tokio::test]
async fn web_search_success_path_returns_inline_sources() {
    let service = SearchService::fake_with_sources();

    let output = service
        .web_search(WebSearchInput {
            query: "ping".to_string(),
            platform: None,
            model: None,
            extra_sources: Some(1),
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(!output.fallback_used);
    assert_eq!(output.sources.len(), output.sources_count);
    assert!(output.sources_count >= 1);
}

struct FailingSourceProvider;

#[async_trait]
impl SourceProvider for FailingSourceProvider {
    async fn search_sources(
        &self,
        _query: &str,
        _max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
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
    async fn search_sources(
        &self,
        _query: &str,
        max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
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

    let output = service
        .web_fetch("https://example.com/article", None)
        .await
        .expect("fetch output");

    assert!(output.content.contains("firecrawl fallback content"));
    assert!(!output.truncated);
    assert_eq!(output.original_length, output.content.chars().count());
}

#[tokio::test]
async fn web_fetch_truncates_to_max_chars_when_explicit() {
    let service = SearchService::fake_with_sources();

    let output = service
        .web_fetch("https://example.com/large", Some(10))
        .await
        .expect("fetch output");

    assert!(output.truncated);
    assert_eq!(output.content.chars().count(), 10);
    assert!(output.original_length > 10);
}

struct LongFetchSourceProvider;

#[async_trait]
impl SourceProvider for LongFetchSourceProvider {
    async fn search_sources(
        &self,
        _query: &str,
        _max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }

    async fn fetch(&self, _url: &str) -> Result<String> {
        Ok("abcdefghijklmnop".to_string())
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn web_fetch_truncates_via_env_default() {
    let service = SearchService::fake_custom(
        None,
        Arc::new(LongFetchSourceProvider),
        None,
        [("GROK_SEARCH_FETCH_MAX_CHARS", "5")],
    );

    let output = service
        .web_fetch("https://example.com/page", None)
        .await
        .expect("fetch output");

    assert!(output.truncated);
    assert_eq!(output.content.chars().count(), 5);
    assert_eq!(output.original_length, 16);
}

#[tokio::test]
async fn get_sources_returns_same_payload_repeatedly() {
    let service = SearchService::fake_with_sources();

    let first = service
        .web_search(WebSearchInput {
            query: "ping".to_string(),
            extra_sources: Some(2),
            ..Default::default()
        })
        .await
        .expect("search output");

    let a = service.get_sources(&first.session_id).await.expect("a");
    let b = service.get_sources(&first.session_id).await.expect("b");
    assert_eq!(a.sources_count, b.sources_count);
    assert_eq!(a.sources, b.sources);
}

struct VerifiableAiProvider;

#[async_trait]
impl AiProvider for VerifiableAiProvider {
    async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
        Ok(SearchResponse {
            content: "verified answer".to_string(),
            sources: vec![Source::new("https://primary.example/a", "grok_responses")],
        })
    }
}

#[tokio::test]
async fn web_search_speculation_serves_enrichment_with_one_source_call() {
    let provider = CountingSourceProvider::default();
    let calls = provider.search_calls.clone();
    let service = SearchService::fake_custom(
        Some(Arc::new(VerifiableAiProvider)),
        Arc::new(provider),
        None,
        [
            ("GROK_SEARCH_EXTRA_SOURCES", "2"),
            ("GROK_SEARCH_FALLBACK_SOURCES", "5"),
        ],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "speculation".to_string(),
            extra_sources: None,
            ..Default::default()
        })
        .await
        .expect("output");

    assert_eq!(output.search_provider, "grok_responses");
    assert!(!output.fallback_used);
    assert_eq!(*calls.lock().expect("lock"), 1);
    // speculative call returned 5; truncated to extra_sources=2; merged with 1 grok = 3
    assert_eq!(output.sources_count, 3);
    assert!(output
        .sources
        .iter()
        .any(|s| s.provider == "tavily_enrichment"));
}
