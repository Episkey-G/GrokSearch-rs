use grok_search_rs::model::search::SearchFilters;
use grok_search_rs::providers::exa::{
    exa_search_request_body, normalize_exa_results, parse_exa_contents, start_published_date,
};
use serde_json::json;

#[test]
fn search_body_carries_native_domain_and_date_filters() {
    let filters = SearchFilters {
        recency_days: Some(30),
        include_domains: vec!["arxiv.org".to_string()],
        exclude_domains: vec!["reddit.com".to_string()],
    };
    // 2026-08-02T00:00:00Z = 1785628800; 30 days earlier is 2026-07-03.
    let body = exa_search_request_body("LLM eval papers", 5, &filters, 1_785_628_800);
    assert_eq!(body["query"], "LLM eval papers");
    assert_eq!(body["numResults"], 5);
    assert_eq!(body["includeDomains"], json!(["arxiv.org"]));
    assert_eq!(body["excludeDomains"], json!(["reddit.com"]));
    assert_eq!(body["startPublishedDate"], "2026-07-03T00:00:00.000Z");
}

#[test]
fn search_body_omits_absent_filters_and_clamps_num_results() {
    let body = exa_search_request_body("q", 0, &SearchFilters::default(), 1_785_628_800);
    assert_eq!(body["numResults"], 1);
    assert!(body.get("includeDomains").is_none());
    assert!(body.get("excludeDomains").is_none());
    assert!(body.get("startPublishedDate").is_none());

    let body = exa_search_request_body("q", 500, &SearchFilters::default(), 1_785_628_800);
    assert_eq!(body["numResults"], 100);
}

#[test]
fn start_published_date_handles_month_and_year_boundaries() {
    // 2026-01-10 minus 15 days = 2025-12-26 (year boundary).
    // 1768003200 = 2026-01-10T00:00:00Z.
    assert_eq!(
        start_published_date(15, 1_768_003_200),
        "2025-12-26T00:00:00.000Z"
    );
    // 2026-03-01 minus 1 day = 2026-02-28 (2026 is not a leap year).
    // 1772323200 = 2026-03-01T00:00:00Z.
    assert_eq!(
        start_published_date(1, 1_772_323_200),
        "2026-02-28T00:00:00.000Z"
    );
    // 2024-03-01 minus 1 day = 2024-02-29 (leap year).
    // 1709251200 = 2024-03-01T00:00:00Z.
    assert_eq!(
        start_published_date(1, 1_709_251_200),
        "2024-02-29T00:00:00.000Z"
    );
}

#[test]
fn search_results_parse_metadata() {
    let raw = json!({
        "results": [
            {
                "title": "A Comprehensive Overview of Large Language Models",
                "url": "https://arxiv.org/pdf/2307.06435.pdf",
                "publishedDate": "2023-11-16T01:36:32.547Z",
                "author": "Humza Naveed",
                "id": "https://arxiv.org/abs/2307.06435"
            },
            { "id": "no-url-dropped" }
        ],
        "requestId": "abc"
    });
    let sources = normalize_exa_results(&raw);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].url, "https://arxiv.org/pdf/2307.06435.pdf");
    assert_eq!(sources[0].provider, "exa");
    assert_eq!(
        sources[0].published_date.as_deref(),
        Some("2023-11-16T01:36:32.547Z")
    );
}

#[test]
fn contents_parse_text_title_and_date() {
    let raw = json!({
        "results": [
            {
                "url": "https://example.com/post",
                "title": "Post Title",
                "publishedDate": "2026-05-01T00:00:00.000Z",
                "text": "Full page text."
            }
        ]
    });
    let page = parse_exa_contents(&raw, "https://example.com/post").expect("page");
    assert_eq!(page.content, "Full page text.");
    assert_eq!(page.title.as_deref(), Some("Post Title"));
    assert_eq!(
        page.published_date.as_deref(),
        Some("2026-05-01T00:00:00.000Z")
    );
}

#[test]
fn contents_failure_surfaces_status_reason() {
    let raw = json!({
        "results": [],
        "statuses": [ { "id": "https://gone.example", "status": "error", "error": { "tag": "CRAWL_NOT_FOUND" } } ]
    });
    let err = parse_exa_contents(&raw, "https://gone.example").expect_err("must fail");
    let message = err.to_string();
    assert!(
        message.contains("CRAWL_NOT_FOUND") && message.contains("https://gone.example"),
        "error must carry the upstream reason and url: {message}"
    );
}
