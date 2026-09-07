use grok_search_rs::providers::firecrawl::{normalize_firecrawl_results, parse_firecrawl_scrape};
use serde_json::json;

#[test]
fn search_parses_v2_web_results() {
    let raw = json!({
        "success": true,
        "data": {
            "web": [
                {
                    "url": "https://example.com/post",
                    "title": "Post Title",
                    "description": "A useful snippet.",
                    "published_date": "2026-09-01",
                    "position": 1
                },
                { "title": "Missing URL" }
            ],
            "news": [{ "url": "https://example.com/news" }],
            "images": [{ "url": "https://example.com/image.png" }]
        }
    });

    let sources = normalize_firecrawl_results(&raw);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].url, "https://example.com/post");
    assert_eq!(sources[0].provider, "firecrawl");
    assert_eq!(sources[0].title.as_deref(), Some("Post Title"));
    assert_eq!(sources[0].description.as_deref(), Some("A useful snippet."));
    assert_eq!(sources[0].published_date.as_deref(), Some("2026-09-01"));
}

#[test]
fn search_preserves_v1_and_flat_response_compatibility() {
    let results = json!([
        { "url": "https://example.com/markdown", "markdown": "# Markdown" },
        { "url": "https://example.com/content", "content": "Plain content." },
        "https://example.com/bare",
        { "title": "Missing URL" }
    ]);

    for raw in [json!({ "data": results }), json!({ "results": results })] {
        let sources = normalize_firecrawl_results(&raw);
        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0].description.as_deref(), Some("# Markdown"));
        assert_eq!(sources[1].description.as_deref(), Some("Plain content."));
        assert_eq!(sources[2].url, "https://example.com/bare");
        assert_eq!(sources[2].provider, "firecrawl");
    }
}

#[test]
fn search_handles_empty_or_missing_web_results() {
    for raw in [
        json!({ "data": { "web": [] } }),
        json!({ "data": { "news": [{ "url": "https://example.com/news" }] } }),
        json!({ "data": { "web": null } }),
        json!({ "data": [] }),
        json!({}),
    ] {
        assert!(normalize_firecrawl_results(&raw).is_empty(), "input: {raw}");
    }
}

// Firecrawl scrape responses carry a rich `data.metadata` object next to the
// markdown (`title`, `publishedTime`, `article:published_time`, OG tags, …
// verified live against api.firecrawl.dev). Title and published date must
// survive parsing so enrichment-time backfill (issue #21) can use them.
#[test]
fn scrape_parses_content_title_and_published_time() {
    let raw = serde_json::json!({
        "success": true,
        "data": {
            "markdown": "# Post\n\nBody.",
            "metadata": {
                "title": "Post Title | Site",
                "publishedTime": "2026-06-19T06:15:24-08:00",
                "article:published_time": "2026-06-19T06:00:00-08:00",
                "ogTitle": "Post Title"
            }
        }
    });

    let page = parse_firecrawl_scrape(&raw).expect("page");

    assert_eq!(page.content, "# Post\n\nBody.");
    assert_eq!(page.title.as_deref(), Some("Post Title | Site"));
    assert_eq!(
        page.published_date.as_deref(),
        Some("2026-06-19T06:15:24-08:00"),
        "publishedTime wins over article:published_time"
    );
}

#[test]
fn scrape_falls_back_to_article_published_time() {
    let raw = serde_json::json!({
        "data": {
            "markdown": "Body.",
            "metadata": {"article:published_time": "2026-06-19T06:00:00-08:00"}
        }
    });

    let page = parse_firecrawl_scrape(&raw).expect("page");

    assert_eq!(page.title, None);
    assert_eq!(
        page.published_date.as_deref(),
        Some("2026-06-19T06:00:00-08:00")
    );
}

#[test]
fn scrape_flat_shape_without_metadata_yields_bare_page() {
    // Legacy/flat response shape: markdown at the top level, no metadata.
    let raw = serde_json::json!({"markdown": "Body."});

    let page = parse_firecrawl_scrape(&raw).expect("page");

    assert_eq!(page.content, "Body.");
    assert_eq!(page.title, None);
    assert_eq!(page.published_date, None);
}

#[test]
fn scrape_empty_content_still_errors() {
    let raw = serde_json::json!({
        "data": {"markdown": " ", "metadata": {"title": "Has Title"}}
    });

    let err = parse_firecrawl_scrape(&raw).expect_err("empty content must error");
    assert!(err.to_string().contains("empty content"), "got: {err}");
}
