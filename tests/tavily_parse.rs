use grok_search_rs::model::search::SearchFilters;
use grok_search_rs::model::source::Source;
use grok_search_rs::providers::tavily::{
    limit_tavily_results, normalize_tavily_results, tavily_map_request_body,
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

#[test]
fn limit_tavily_results_truncates_api_results() {
    let sources = (0..20)
        .map(|idx| Source::new(format!("https://example.com/{idx}"), "tavily"))
        .collect();

    let limited = limit_tavily_results(sources, 5);

    assert_eq!(limited.len(), 5);
    assert_eq!(limited[4].url, "https://example.com/4");
}
