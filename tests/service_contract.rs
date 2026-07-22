use async_trait::async_trait;
use grok_search_rs::error::{GrokSearchError, Result};
use grok_search_rs::model::search::{SearchFilters, SearchRequest, SearchResponse};
use grok_search_rs::model::source::{FetchedPage, Source};
use grok_search_rs::model::tool::WebSearchInput;
use grok_search_rs::service::{AiProvider, SearchService, SourceProvider};
use grok_search_rs::sources::{SourceCaps, SourceExtractor, SourceRouter, SourceType};
use reqwest::Client;
use std::sync::{Arc, Mutex};
use url::Url;

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
        .get_sources(&output.session_id, 0, None)
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

    async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
        Ok(FetchedPage::text("fetched"))
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
        .get_sources(&output.session_id, 0, None)
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

struct TimeoutAiProvider;

#[async_trait]
impl AiProvider for TimeoutAiProvider {
    async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
        Err(GrokSearchError::Timeout("upstream timed out".to_string()))
    }
}

struct ProviderErrAiProvider;

#[async_trait]
impl AiProvider for ProviderErrAiProvider {
    async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
        Err(GrokSearchError::Provider("HTTP 500".to_string()))
    }
}

// A failing Grok call surfaces the specific error kind as fallback_reason
// (timeout vs the legacy generic provider error), so degraded responses are
// diagnosable rather than collapsed into one catch-all code.
#[tokio::test]
async fn web_search_fallback_reason_reflects_grok_error_kind() {
    let timeout_service = SearchService::fake_custom(
        Some(Arc::new(TimeoutAiProvider)),
        Arc::new(CountingSourceProvider::default()),
        None,
        [] as [(&str, &str); 0],
    );
    let timeout_output = timeout_service
        .web_search(WebSearchInput {
            query: "anything".to_string(),
            ..Default::default()
        })
        .await
        .expect("fallback output");
    assert!(timeout_output.fallback_used);
    assert_eq!(
        timeout_output.fallback_reason,
        Some("grok_timeout".to_string())
    );

    let provider_service = SearchService::fake_custom(
        Some(Arc::new(ProviderErrAiProvider)),
        Arc::new(CountingSourceProvider::default()),
        None,
        [] as [(&str, &str); 0],
    );
    let provider_output = provider_service
        .web_search(WebSearchInput {
            query: "anything".to_string(),
            ..Default::default()
        })
        .await
        .expect("fallback output");
    assert!(provider_output.fallback_used);
    assert_eq!(
        provider_output.fallback_reason,
        Some("grok_provider_error".to_string())
    );
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
        .get_sources(&output.session_id, 0, None)
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
async fn fallback_honors_include_content_false() {
    // include_content=false is an explicit opt-out and must suppress inline
    // enrichment even on the degraded source-fallback path (P2), so the extra
    // fetch budget is not spent for callers who disabled inline content.
    let service = SearchService::fake_custom(
        Some(Arc::new(EmptySourcesAiProvider)),
        Arc::new(CountingSourceProvider::default()),
        None,
        [("GROK_SEARCH_FALLBACK_SOURCES", "3")],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            include_content: Some(false),
            ..Default::default()
        })
        .await
        .expect("fallback output");

    assert_eq!(output.search_provider, "source_fallback");
    assert!(output.fallback_used);
    assert!(!output.sources.is_empty());
    for s in &output.sources {
        assert!(
            s.content.is_none(),
            "include_content=false must leave fallback content empty, got: {:?}",
            s.content
        );
    }
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

    async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
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

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        Ok(FetchedPage::text(format!(
            "firecrawl fallback content for {url}"
        )))
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

/// Models the primary (Tavily) leg yielding nothing usable — e.g. every hit
/// discarded by the relevance-score filter, or a genuinely empty result.
struct EmptySearchSourceProvider;

#[async_trait]
impl SourceProvider for EmptySearchSourceProvider {
    async fn search_sources(
        &self,
        _query: &str,
        _max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }

    async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
        Ok(FetchedPage::text("empty provider fetch"))
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

// A filter-constrained request must never receive supplemental sources from
// the filter-blind fallback provider: Firecrawl ignores SearchFilters, so
// falling through would silently drop the caller's include/exclude/recency
// constraints. Constrained requests are Tavily-or-nothing.
#[tokio::test]
async fn constrained_search_never_falls_through_to_filter_blind_fallback() {
    let service = SearchService::fake_custom(
        None,
        Arc::new(EmptySearchSourceProvider),
        Some(Arc::new(FirecrawlLikeSourceProvider)),
        [] as [(&str, &str); 0],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "rmcp Rust MCP SDK release".to_string(),
            include_domains: vec!["github.com".to_string()],
            include_content: Some(false),
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(!output.fallback_used);
    assert!(
        output
            .sources
            .iter()
            .all(|source| !source.url.contains("firecrawl.example")),
        "constrained request must not surface filter-blind fallback sources: {:?}",
        output.sources
    );
}

// Unconstrained requests keep the legacy resilience: an empty primary result
// still falls through to the fallback provider for enrichment.
#[tokio::test]
async fn unconstrained_search_still_falls_through_to_fallback_enrichment() {
    let service = SearchService::fake_custom(
        None,
        Arc::new(EmptySearchSourceProvider),
        Some(Arc::new(FirecrawlLikeSourceProvider)),
        [] as [(&str, &str); 0],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "rmcp Rust MCP SDK release".to_string(),
            include_content: Some(false),
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(!output.fallback_used);
    assert!(
        output
            .sources
            .iter()
            .any(|source| source.provider == "firecrawl_enrichment"
                && source.url.contains("firecrawl.example")),
        "unconstrained request must still enrich via the fallback provider: {:?}",
        output.sources
    );
}

struct AlwaysErrExtractor;

#[async_trait]
impl SourceExtractor for AlwaysErrExtractor {
    fn matches(&self, _url: &Url) -> bool {
        true
    }
    fn kind(&self) -> SourceType {
        SourceType::GithubIssue
    }
    async fn fetch_render(&self, _c: &Client, _u: &Url, _caps: &SourceCaps) -> Result<String> {
        Err(GrokSearchError::Provider("injected boom".to_string()))
    }
}

struct AlwaysEmptyExtractor;

#[async_trait]
impl SourceExtractor for AlwaysEmptyExtractor {
    fn matches(&self, _url: &Url) -> bool {
        true
    }
    fn kind(&self) -> SourceType {
        SourceType::GithubIssue
    }
    async fn fetch_render(&self, _c: &Client, _u: &Url, _caps: &SourceCaps) -> Result<String> {
        Ok(String::new())
    }
}

// Success criterion 2: always-Err specialist → graceful generic fallback.
#[tokio::test]
async fn web_fetch_specialist_err_falls_back_to_generic() {
    let service = SearchService::fake_with_router(
        Arc::new(FailingSourceProvider),
        Some(Arc::new(FirecrawlLikeSourceProvider)),
        SourceRouter::with_extractors(vec![Box::new(AlwaysErrExtractor)]),
    );

    let output = service
        .web_fetch("https://github.com/owner/repo/issues/1", None)
        .await
        .expect("fetch must succeed via generic fallback");

    assert!(output.content.contains("firecrawl fallback content"));
    assert_eq!(output.source_type, SourceType::Generic); // D-02
    assert!(output.fallback_reason.is_some()); // D-01: specialist matched then failed
}

// Success criterion 3: empty render → treated as failure → generic fallback.
#[tokio::test]
async fn web_fetch_specialist_empty_render_falls_back_to_generic() {
    let service = SearchService::fake_with_router(
        Arc::new(FailingSourceProvider),
        Some(Arc::new(FirecrawlLikeSourceProvider)),
        SourceRouter::with_extractors(vec![Box::new(AlwaysEmptyExtractor)]),
    );

    let output = service
        .web_fetch("https://github.com/owner/repo/issues/2", None)
        .await
        .expect("fetch must succeed via generic fallback");

    assert!(output.content.contains("firecrawl fallback content"));
    assert_eq!(output.source_type, SourceType::Generic);
    let reason = output
        .fallback_reason
        .expect("empty render must surface a reason");
    assert!(reason.contains("empty render"), "got: {reason}");
}

// D-01: a no-match URL goes generic silently — no fallback_reason.
#[tokio::test]
async fn web_fetch_no_specialist_match_has_no_fallback_reason() {
    let service = SearchService::fake_with_router(
        Arc::new(FailingSourceProvider),
        Some(Arc::new(FirecrawlLikeSourceProvider)),
        SourceRouter::default(), // empty router → never matches
    );

    let output = service
        .web_fetch("https://example.com/article", None)
        .await
        .expect("fetch output");

    assert_eq!(output.source_type, SourceType::Generic);
    assert!(output.fallback_reason.is_none());
}

// Success criterion 1: source_type is always present (generic in Phase 1).
#[tokio::test]
async fn web_fetch_source_type_present_on_normal_fetch() {
    let service = SearchService::fake_with_sources();

    let output = service
        .web_fetch("https://example.com/page", None)
        .await
        .expect("fetch output");

    assert_eq!(output.source_type, SourceType::Generic);
    assert!(output.fallback_reason.is_none());
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

    async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
        Ok(FetchedPage::text("abcdefghijklmnop"))
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

    let a = service
        .get_sources(&first.session_id, 0, None)
        .await
        .expect("a");
    let b = service
        .get_sources(&first.session_id, 0, None)
        .await
        .expect("b");
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
