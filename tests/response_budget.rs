//! Agent-context-budget contract for web_search / get_sources responses.
//!
//! Mirrors the patterns used by Tavily MCP (snippet-first), Exa MCP
//! (highlights + "follow up with web_fetch"), and the official MCP fetch
//! server (max_length + start_index continuation): the first response stays
//! small, drill-down is explicit, and every truncation tells the agent how to
//! get the rest.

use async_trait::async_trait;
use grok_search_rs::error::{GrokSearchError, Result};
use grok_search_rs::model::search::{SearchFilters, SearchRequest, SearchResponse};
use grok_search_rs::model::source::{FetchedPage, Source};
use grok_search_rs::model::tool::WebSearchInput;
use grok_search_rs::service::{AiProvider, SearchService, SourceProvider};

/// Supplemental provider whose generic fetch returns a body of fixed length,
/// so per-source inline content size is deterministic.
struct FixedLenFetchProvider {
    len: usize,
}

#[async_trait]
impl SourceProvider for FixedLenFetchProvider {
    async fn search_sources(
        &self,
        _query: &str,
        max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        Ok((0..max_results)
            .map(|idx| Source::new(format!("https://example.com/source-{idx}"), "tavily"))
            .collect())
    }

    async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
        Ok(FetchedPage::text("x".repeat(self.len)))
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

fn total_inline_chars(sources: &[Source]) -> usize {
    sources
        .iter()
        .filter_map(|s| s.content.as_deref())
        .map(|c| c.chars().count())
        .sum()
}

// Item 1: only the first GROK_SEARCH_MAX_INLINE_SOURCES merged sources get
// inline content; the rest stay metadata-only (content key absent), bounding
// the response even when Grok returns dozens of citations.
#[tokio::test]
async fn inline_content_capped_to_max_inline_sources() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(FixedLenFetchProvider { len: 100 }),
        None,
        [
            ("GROK_SEARCH_EXTRA_SOURCES", "3"),
            ("GROK_SEARCH_MAX_INLINE_SOURCES", "2"),
        ],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            ..Default::default()
        })
        .await
        .expect("search output");

    // merged = 1 grok citation + 3 supplemental = 4 sources
    assert_eq!(output.sources_count, 4);
    let with_content: Vec<bool> = output.sources.iter().map(|s| s.content.is_some()).collect();
    assert_eq!(
        with_content,
        vec![true, true, false, false],
        "only the first max_inline_sources entries may carry inline content"
    );
}

// Item 2: whole-response budget. The returned payload is trimmed from the
// tail, every trimmed source carries an actionable note (web_fetch /
// get_sources), and the cache keeps the full enriched content so drill-down
// loses nothing.
#[tokio::test]
async fn response_budget_trims_tail_and_cache_keeps_full_content() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(FixedLenFetchProvider { len: 10_000 }),
        None,
        [
            ("GROK_SEARCH_EXTRA_SOURCES", "3"),
            ("GROK_SEARCH_MAX_INLINE_SOURCES", "10"),
            ("GROK_SEARCH_RESPONSE_MAX_CHARS", "12000"),
        ],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(output.truncated, "over-budget response must set truncated");
    assert!(
        total_inline_chars(&output.sources) + output.content.chars().count() <= 12_000,
        "returned payload must respect the response budget, got {}",
        total_inline_chars(&output.sources) + output.content.chars().count()
    );
    // First source survives intact: budget trims from the tail.
    assert_eq!(
        output.sources[0].content.as_deref().map(|c| c.len()),
        Some(10_000),
        "head source must keep full inline content"
    );
    // Trimmed sources tell the agent how to recover the full text.
    let tail_note = output.sources[3]
        .content
        .as_deref()
        .expect("trimmed source keeps a note");
    assert!(tail_note.contains("web_fetch"), "note: {tail_note}");
    assert!(
        tail_note.contains(&output.session_id),
        "note must reference the session for get_sources: {tail_note}"
    );

    // Drill-down: the cache holds the untrimmed content.
    let page = service
        .get_sources(&output.session_id, 3, Some(1))
        .await
        .expect("cached page");
    assert_eq!(page.sources.len(), 1);
    assert_eq!(
        page.sources[0].content.as_deref().map(|c| c.len()),
        Some(10_000),
        "cache must keep full enriched content for drill-down"
    );
}

// Budget never trims when the payload already fits.
#[tokio::test]
async fn response_within_budget_is_not_truncated() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(FixedLenFetchProvider { len: 100 }),
        None,
        [("GROK_SEARCH_EXTRA_SOURCES", "2")],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(!output.truncated);
    assert!(output
        .sources
        .iter()
        .all(|s| { s.content.as_deref().is_some_and(|c| c.len() == 100) }));
}

// Item 3: get_sources pagination — offset/limit slice the cached list and
// next_offset points at the next page (official fetch server's start_index
// pattern, applied to sources).
#[tokio::test]
async fn get_sources_paginates_with_offset_limit() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(FixedLenFetchProvider { len: 50 }),
        None,
        [("GROK_SEARCH_EXTRA_SOURCES", "3")],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            ..Default::default()
        })
        .await
        .expect("search output");
    assert_eq!(output.sources_count, 4);

    let first = service
        .get_sources(&output.session_id, 0, Some(2))
        .await
        .expect("first page");
    assert_eq!(first.sources.len(), 2);
    assert_eq!(first.sources_count, 2);
    assert_eq!(first.total_sources, 4);
    assert_eq!(first.offset, 0);
    assert_eq!(first.next_offset, Some(2));

    let second = service
        .get_sources(&output.session_id, 2, None)
        .await
        .expect("second page");
    assert_eq!(second.sources.len(), 2);
    assert_eq!(second.offset, 2);
    assert_eq!(second.next_offset, None);

    // Offset past the end is an empty page, not an error.
    let past = service
        .get_sources(&output.session_id, 10, None)
        .await
        .expect("past-the-end page");
    assert!(past.sources.is_empty());
    assert_eq!(past.next_offset, None);
    assert_eq!(past.total_sources, 4);
}

/// Provider returning many sources whose metadata alone (long descriptions)
/// outweighs the budget — the real-world shape of a broad survey query where
/// Grok cites 50+ pages.
struct MetadataHeavyProvider {
    desc_len: usize,
}

#[async_trait]
impl SourceProvider for MetadataHeavyProvider {
    async fn search_sources(
        &self,
        _query: &str,
        max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        Ok((0..max_results)
            .map(|idx| {
                let mut s = Source::new(format!("https://example.com/source-{idx}"), "tavily");
                s.description = Some("d".repeat(self.desc_len));
                s
            })
            .collect())
    }

    async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
        Ok(FetchedPage::text("body"))
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

// The budget must track the real payload, metadata included: when dozens of
// metadata-only sources alone exceed it, the returned list is cut from the
// tail (cache keeps everything; get_sources pages the rest), at least one
// source always survives, and sources_count keeps reporting the cache total.
#[tokio::test]
async fn budget_counts_metadata_and_drops_tail_sources() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(MetadataHeavyProvider { desc_len: 500 }),
        None,
        [
            ("GROK_SEARCH_EXTRA_SOURCES", "9"),
            ("GROK_SEARCH_RESPONSE_MAX_CHARS", "2000"),
        ],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            response_format: Some("concise".to_string()),
            ..Default::default()
        })
        .await
        .expect("search output");

    // merged = 1 grok + 9 supplemental = 10 cached sources
    assert_eq!(
        output.sources_count, 10,
        "sources_count reports cache total"
    );
    assert!(output.truncated, "metadata overflow must set truncated");
    assert!(
        output.sources.len() < 10,
        "tail metadata-only sources must be dropped, kept {}",
        output.sources.len()
    );
    assert!(
        !output.sources.is_empty(),
        "at least one source must survive"
    );

    // Nothing is lost: the cache still pages all 10.
    let page = service
        .get_sources(&output.session_id, 0, None)
        .await
        .expect("cached sources");
    assert_eq!(page.total_sources, 10);
}

// Item 4: response_format=concise returns the synthesized answer plus
// metadata-only sources — the Tavily/Exa default shape.
#[tokio::test]
async fn response_format_concise_omits_inline_content() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(FixedLenFetchProvider { len: 100 }),
        None,
        [] as [(&str, &str); 0],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            response_format: Some("concise".to_string()),
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(!output.content.is_empty());
    assert!(output.sources_count > 0);
    assert!(
        output.sources.iter().all(|s| s.content.is_none()),
        "concise must omit inline content"
    );
}

// Explicit response_format wins over the legacy include_content flag.
#[tokio::test]
async fn response_format_detailed_overrides_include_content_false() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(FixedLenFetchProvider { len: 100 }),
        None,
        [] as [(&str, &str); 0],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            include_content: Some(false),
            response_format: Some("detailed".to_string()),
            ..Default::default()
        })
        .await
        .expect("search output");

    assert!(
        output.sources.iter().any(|s| s.content.is_some()),
        "detailed must inline content even when include_content=false"
    );
}

#[tokio::test]
async fn response_format_invalid_value_is_rejected() {
    let service = SearchService::fake_custom(
        None,
        std::sync::Arc::new(FixedLenFetchProvider { len: 100 }),
        None,
        [] as [(&str, &str); 0],
    );

    let err = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            response_format: Some("verbose".to_string()),
            ..Default::default()
        })
        .await
        .expect_err("invalid response_format must be rejected");

    assert!(
        err.to_string().contains("response_format"),
        "error must name the offending parameter: {err}"
    );
}

/// Grok result as the adapters shape it: what the answer rests on is labelled
/// and ranked ahead of the raw search hits.
struct UsedSourcesAiProvider;

#[async_trait]
impl AiProvider for UsedSourcesAiProvider {
    async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
        let used = |url: &str, evidence: &'static str| Source {
            evidence: Some(evidence.into()),
            ..Source::new(url, "grok_responses")
        };
        Ok(SearchResponse {
            content: "answer".to_string(),
            sources: vec![
                used("https://grok.example/cited", "cited"),
                used("https://grok.example/opened", "opened"),
                Source::new("https://grok.example/hit-0", "grok_responses"),
                Source::new("https://grok.example/hit-1", "grok_responses"),
            ],
        })
    }
}

fn used_sources_service(used_sources_only: &str) -> SearchService {
    SearchService::fake_custom(
        Some(std::sync::Arc::new(UsedSourcesAiProvider)),
        std::sync::Arc::new(FixedLenFetchProvider { len: 50 }),
        None,
        [
            ("GROK_SEARCH_EXTRA_SOURCES", "2"),
            ("GROK_SEARCH_USED_SOURCES_ONLY", used_sources_only),
        ],
    )
}

// Supplemental sources follow what the answer rests on instead of trailing the
// raw search hits, and GROK_SEARCH_USED_SOURCES_ONLY returns exactly that head
// while the session cache keeps everything for get_sources.
#[tokio::test]
async fn used_sources_only_returns_the_labelled_head_and_caches_the_rest() {
    let query = || WebSearchInput {
        query: "q".to_string(),
        response_format: Some("concise".to_string()),
        ..Default::default()
    };
    let urls = |sources: &[Source]| sources.iter().map(|s| s.url.clone()).collect::<Vec<_>>();
    let head = [
        "https://grok.example/cited",
        "https://grok.example/opened",
        "https://example.com/source-0",
        "https://example.com/source-1",
    ];
    let everything = [
        &head[..],
        &["https://grok.example/hit-0", "https://grok.example/hit-1"],
    ]
    .concat();

    let full = used_sources_service("false")
        .web_search(query())
        .await
        .expect("search output");
    assert_eq!(urls(&full.sources), everything);

    let service = used_sources_service("true");
    let output = service.web_search(query()).await.expect("search output");
    assert_eq!(urls(&output.sources), head);
    assert_eq!(output.sources_count, everything.len());

    let cached = service
        .get_sources(&output.session_id, 0, None)
        .await
        .expect("cached sources");
    assert_eq!(urls(&cached.sources), everything);
    assert_eq!(cached.sources[0].evidence.as_deref(), Some("cited"));
}

// The filtered list is a prefix of the cache, so every budget note's
// get_sources offset still names the source it was written for.
#[tokio::test]
async fn used_sources_only_keeps_budget_note_offsets_valid() {
    let service = SearchService::fake_custom(
        Some(std::sync::Arc::new(UsedSourcesAiProvider)),
        std::sync::Arc::new(FixedLenFetchProvider { len: 2_000 }),
        None,
        [
            ("GROK_SEARCH_EXTRA_SOURCES", "2"),
            ("GROK_SEARCH_USED_SOURCES_ONLY", "true"),
            ("GROK_SEARCH_RESPONSE_MAX_CHARS", "3000"),
        ],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            response_format: Some("detailed".to_string()),
            ..Default::default()
        })
        .await
        .expect("search output");
    assert!(output.truncated);

    let cached = service
        .get_sources(&output.session_id, 0, None)
        .await
        .expect("cached sources");
    let noted: Vec<_> = output
        .sources
        .iter()
        .filter_map(|source| {
            let offset = source
                .content
                .as_deref()?
                .split("offset=")
                .nth(1)?
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<usize>()
                .ok()?;
            Some((source.url.as_str(), offset))
        })
        .collect();
    assert!(!noted.is_empty(), "the budget must leave recovery notes");
    for (url, offset) in noted {
        assert_eq!(cached.sources[offset].url, url);
    }
}

struct FailingAiProvider;

#[async_trait]
impl AiProvider for FailingAiProvider {
    async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
        Err(GrokSearchError::Provider("HTTP 500".to_string()))
    }
}

// The switch filters only a successful Grok answer: fallback sources carry no
// `evidence` and must all come back when Grok fails.
#[tokio::test]
async fn used_sources_only_leaves_the_fallback_path_alone() {
    let service = SearchService::fake_custom(
        Some(std::sync::Arc::new(FailingAiProvider)),
        std::sync::Arc::new(FixedLenFetchProvider { len: 50 }),
        None,
        [("GROK_SEARCH_USED_SOURCES_ONLY", "true")],
    );

    let output = service
        .web_search(WebSearchInput {
            query: "q".to_string(),
            response_format: Some("concise".to_string()),
            ..Default::default()
        })
        .await
        .expect("fallback output");

    assert!(output.fallback_used);
    assert_eq!(output.sources.len(), 5);
}
