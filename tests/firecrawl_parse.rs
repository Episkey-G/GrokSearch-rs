use grok_search_rs::providers::firecrawl::parse_firecrawl_scrape;

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
