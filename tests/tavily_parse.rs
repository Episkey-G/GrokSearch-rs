use grok_search_rs::model::search::SearchFilters;
use grok_search_rs::model::source::Source;
use grok_search_rs::providers::tavily::{
    limit_tavily_results, normalize_tavily_results, parse_tavily_extract, tavily_map_request_body,
    tavily_search_request_body,
};

#[test]
fn normalizes_tavily_map_string_results() {
    let raw = serde_json::json!({
        "base_url": "https://openai.com",
        "results": [
            "https://openai.com/",
            "https://platform.openai.com/"
        ]
    });

    let sources = normalize_tavily_results(&raw);

    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].url, "https://openai.com/");
    assert_eq!(sources[0].provider, "tavily");
}

// Regression for the "latest ..." keyword-drift incident (2026-07-22): for
// the query "latest rmcp Rust MCP SDK release version and what changed",
// Tavily returned dictionary/news-portal pages about the word "latest" at
// scores ≤ 0.04, and all of them shipped as `tavily_enrichment` sources.
// On-topic results score ≥ 0.49 live; low-scored items must be dropped.
#[test]
fn normalize_drops_search_results_below_min_relevance_score() {
    let raw = serde_json::json!({
        "results": [
            {"url": "https://www.wordwebonline.com/en/LATEST", "title": "latest, late, latests- WordWeb dictionary definition", "score": 0.03967239},
            {"url": "https://github.com/modelcontextprotocol/rust-sdk/releases", "title": "Releases · modelcontextprotocol/rust-sdk · GitHub", "score": 0.79580134},
            {"url": "https://www.nytimes.com", "title": "The New York Times - Breaking News", "score": 0.03402659},
            {"url": "https://docs.rs/crate/rmcp/latest", "title": "rmcp - Docs.rs", "score": 0.71137124}
        ]
    });

    let sources = normalize_tavily_results(&raw);

    let urls: Vec<&str> = sources.iter().map(|source| source.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://github.com/modelcontextprotocol/rust-sdk/releases",
            "https://docs.rs/crate/rmcp/latest"
        ],
        "junk-scored results must be dropped, order of survivors preserved"
    );
}

#[test]
fn normalize_keeps_results_at_threshold_or_without_score() {
    let raw = serde_json::json!({
        "results": [
            {"url": "https://example.com/at-threshold", "score": 0.1},
            {"url": "https://example.com/no-score", "title": "Score-less item"}
        ]
    });

    let sources = normalize_tavily_results(&raw);

    assert_eq!(
        sources.len(),
        2,
        "cutoff is strict less-than; missing score must fail open"
    );
    assert_eq!(sources[0].url, "https://example.com/at-threshold");
    assert_eq!(sources[1].url, "https://example.com/no-score");
}

#[test]
fn tavily_map_request_uses_limit_not_max_results() {
    let body = tavily_map_request_body("https://openai.com/news/", 5);

    assert_eq!(body["url"], "https://openai.com/news/");
    assert_eq!(body["max_depth"], 1);
    assert_eq!(body["limit"], 5);
    assert!(body.get("max_results").is_none());
}

#[test]
fn tavily_search_body_omits_filters_when_empty() {
    let body = tavily_search_request_body("rust async", 4, &SearchFilters::default());

    assert_eq!(body["query"], "rust async");
    assert_eq!(body["max_results"], 4);
    assert_eq!(body["include_answer"], false);
    assert!(body.get("days").is_none());
    assert!(body.get("topic").is_none());
    assert!(body.get("include_domains").is_none());
    assert!(body.get("exclude_domains").is_none());
}

#[test]
fn tavily_search_body_serializes_filters() {
    let filters = SearchFilters {
        recency_days: Some(3),
        include_domains: vec!["github.com".to_string(), "news.ycombinator.com".to_string()],
        exclude_domains: vec!["example.com".to_string()],
    };

    let body = tavily_search_request_body("today AI", 5, &filters);

    assert_eq!(body["days"], 3);
    assert_eq!(body["topic"], "news");
    assert_eq!(
        body["include_domains"],
        serde_json::json!(["github.com", "news.ycombinator.com"])
    );
    assert_eq!(body["exclude_domains"], serde_json::json!(["example.com"]));
}

// The extract endpoint returns `title` alongside `raw_content` (verified live
// against api.tavily.com — the docs' sample response omits it) but no
// published-date field. Metadata must survive parsing so enrichment-time
// backfill (issue #21) can use it.
#[test]
fn extract_parses_content_and_title() {
    let raw = serde_json::json!({
        "results": [
            {
                "url": "https://example.com/post",
                "raw_content": "# Heading\n\nBody.",
                "title": "Example Post Title",
                "images": []
            }
        ],
        "failed_results": []
    });

    let page = parse_tavily_extract(&raw).expect("page");

    assert_eq!(page.content, "# Heading\n\nBody.");
    assert_eq!(page.title.as_deref(), Some("Example Post Title"));
    assert_eq!(page.published_date, None);
}

#[test]
fn extract_tolerates_missing_or_blank_title() {
    let raw = serde_json::json!({
        "results": [
            {"url": "https://example.com/a", "raw_content": "Body.", "title": "   "}
        ]
    });

    let page = parse_tavily_extract(&raw).expect("page");

    assert_eq!(page.content, "Body.");
    assert_eq!(page.title, None, "blank titles must normalize to None");
}

#[test]
fn extract_empty_content_still_errors() {
    let raw = serde_json::json!({
        "results": [
            {"url": "https://example.com/a", "raw_content": "  ", "title": "Has Title"}
        ]
    });

    let err = parse_tavily_extract(&raw).expect_err("empty content must error");
    assert!(err.to_string().contains("empty content"), "got: {err}");
}

#[test]
fn limit_tavily_results_truncates_api_results() {
    let sources = (0..20)
        .map(|idx| Source::new(format!("https://example.com/{idx}"), "tavily"))
        .collect();

    let limited = limit_tavily_results(sources, 5);

    assert_eq!(limited.len(), 5);
    assert_eq!(limited[4].url, "https://example.com/4");
}
