use grok_search_rs::model::search::SearchFilters;
use grok_search_rs::providers::tinyfish::{
    normalize_tinyfish_results, parse_tinyfish_fetch, tinyfish_search_params,
};
use serde_json::json;

#[test]
fn search_results_parse_title_snippet_url_and_date() {
    let raw = json!({
        "query": "web automation tools",
        "results": [
            {
                "position": 1,
                "site_name": "tinyfish.ai",
                "title": "TinyFish — AI Web Automation Platform",
                "snippet": "Automate any website...",
                "url": "https://tinyfish.ai"
            },
            {
                "position": 2,
                "title": "News item",
                "url": "https://example.com/news",
                "date": "2026-07-30"
            },
            { "position": 3, "title": "no url, dropped" }
        ],
        "total_results": 3,
        "page": 0
    });
    let sources = normalize_tinyfish_results(&raw);
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].url, "https://tinyfish.ai");
    assert_eq!(sources[0].provider, "tinyfish");
    assert_eq!(
        sources[0].title.as_deref(),
        Some("TinyFish — AI Web Automation Platform")
    );
    assert_eq!(
        sources[0].description.as_deref(),
        Some("Automate any website...")
    );
    assert_eq!(sources[1].published_date.as_deref(), Some("2026-07-30"));
}

// Domain filters ride the dedicated comma-separated params, never the query
// string: `site:`/`-site:` operators are deprecated for domain filtering
// upstream because they collide with other query syntax, and the caller's
// query is arbitrary user text.
#[test]
fn domain_filters_use_dedicated_params_and_leave_the_query_untouched() {
    let filters = SearchFilters {
        recency_days: None,
        include_domains: vec!["docs.rs".to_string(), "crates.io".to_string()],
        exclude_domains: vec!["pinterest.com".to_string()],
    };
    let params = tinyfish_search_params("rust http client", &filters);
    assert_eq!(
        params,
        vec![
            ("query", "rust http client".to_string()),
            ("include_domains", "docs.rs,crates.io".to_string()),
            ("exclude_domains", "pinterest.com".to_string()),
        ]
    );
}

// A query carrying operator-like syntax of its own must survive verbatim —
// the old string-splicing path could turn it into a different query.
#[test]
fn operator_like_query_text_is_not_rewritten() {
    let filters = SearchFilters {
        recency_days: None,
        include_domains: vec!["github.com".to_string()],
        exclude_domains: Vec::new(),
    };
    let params = tinyfish_search_params("rust OR go site:example.com", &filters);
    assert_eq!(
        params[0],
        ("query", "rust OR go site:example.com".to_string())
    );
    assert_eq!(params[1], ("include_domains", "github.com".to_string()));
}

#[test]
fn recency_days_map_to_minutes_with_ten_year_cap() {
    let week = SearchFilters {
        recency_days: Some(7),
        include_domains: Vec::new(),
        exclude_domains: Vec::new(),
    };
    let params = tinyfish_search_params("q", &week);
    assert_eq!(params[1], ("recency_minutes", (7 * 24 * 60).to_string()));

    let huge = SearchFilters {
        recency_days: Some(4_000_000),
        include_domains: Vec::new(),
        exclude_domains: Vec::new(),
    };
    let params = tinyfish_search_params("q", &huge);
    assert_eq!(params[1], ("recency_minutes", "5256000".to_string()));
}

#[test]
fn unfiltered_query_sends_only_the_query_param() {
    let params = tinyfish_search_params("plain", &SearchFilters::default());
    assert_eq!(params, vec![("query", "plain".to_string())]);
}

#[test]
fn fetch_parses_markdown_text_and_title() {
    let raw = json!({
        "results": [
            {
                "url": "https://www.tinyfish.ai/",
                "final_url": "https://www.tinyfish.ai/",
                "title": "TinyFish | Enterprise Web Agent Infrastructure",
                "description": "TinyFish provides...",
                "language": "en",
                "format": "markdown",
                "text": "# TinyFish\n\nEnterprise infrastructure..."
            }
        ],
        "errors": []
    });
    let page = parse_tinyfish_fetch(&raw, "https://www.tinyfish.ai/").expect("page");
    assert_eq!(page.content, "# TinyFish\n\nEnterprise infrastructure...");
    assert_eq!(
        page.title.as_deref(),
        Some("TinyFish | Enterprise Web Agent Infrastructure")
    );
    assert_eq!(page.published_date, None);
}

#[test]
fn fetch_surfaces_per_url_error_from_errors_array() {
    let raw = json!({
        "results": [],
        "errors": [ { "url": "https://blocked.example", "error": "anti_bot_challenge" } ]
    });
    let err = parse_tinyfish_fetch(&raw, "https://blocked.example").expect_err("must fail");
    let message = err.to_string();
    assert!(
        message.contains("anti_bot_challenge") && message.contains("https://blocked.example"),
        "error must carry the upstream reason and url: {message}"
    );
}

#[test]
fn fetch_with_empty_text_is_an_error_not_a_blank_page() {
    let raw = json!({ "results": [ { "url": "https://x.example", "text": "   " } ], "errors": [] });
    assert!(parse_tinyfish_fetch(&raw, "https://x.example").is_err());
}
