use grok_search_rs::adapters::sources::rank_used_first;
use grok_search_rs::model::source::{merge_sources, Source};

#[test]
fn merge_sources_dedupes_by_url_and_preserves_first_provider() {
    let xai = Source::new("https://openai.com/news", "grok_responses").with_title("OpenAI News");
    let tavily = Source::new("https://openai.com/news", "tavily").with_title("Duplicate");
    let other = Source::new("https://example.com/a", "tavily");

    let merged = merge_sources(vec![xai], vec![tavily, other]);

    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].provider, "grok_responses");
    assert_eq!(merged[0].title.as_deref(), Some("OpenAI News"));
    assert_eq!(merged[1].url, "https://example.com/a");
}

#[test]
fn source_provider_field_accepts_static_str_via_cow() {
    let source = Source::new("https://example.com", "tavily");
    // Pin the contract: Cow<'static, str> compares equal to &str literals so
    // downstream assertions like `source.provider == "tavily"` keep working.
    assert_eq!(source.provider, "tavily");
}

#[test]
fn rank_used_first_leads_in_used_order_and_keeps_the_rest_stable() {
    let mut sources = vec![
        Source::new("https://example.com/a", "grok_responses").with_title("A"),
        Source::new("https://example.com/b", "grok_responses"),
        Source::new("https://example.com/c", "grok_responses"),
        Source::new("https://example.com/d", "grok_responses"),
    ];
    // Duplicates and URLs absent from `sources` must not disturb the order.
    let cited = [
        Source::new("https://example.com/d", "grok_responses"),
        Source::new("https://example.com/b", "grok_responses"),
        Source::new("https://example.com/b", "grok_responses"),
        Source::new("https://example.com/missing", "grok_responses"),
    ];
    // A page both cited and opened ranks and is labelled as cited.
    let opened = [
        Source::new("https://example.com/c", "grok_responses"),
        Source::new("https://example.com/d", "grok_responses"),
    ];

    rank_used_first(&mut sources, &cited, &opened);

    let urls: Vec<_> = sources.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://example.com/d",
            "https://example.com/b",
            "https://example.com/c",
            "https://example.com/a",
        ]
    );
    let evidence: Vec<_> = sources.iter().map(|s| s.evidence.as_deref()).collect();
    assert_eq!(
        evidence,
        [Some("cited"), Some("cited"), Some("opened"), None]
    );
    assert_eq!(sources[3].title.as_deref(), Some("A"));
}

#[test]
fn rank_used_first_without_used_sources_keeps_the_order() {
    let mut sources = vec![
        Source::new("https://example.com/b", "grok_responses"),
        Source::new("https://example.com/a", "grok_responses"),
    ];

    rank_used_first(&mut sources, &[], &[]);

    assert_eq!(sources[0].url, "https://example.com/b");
    assert_eq!(sources[1].url, "https://example.com/a");
    assert!(sources.iter().all(|s| s.evidence.is_none()));
}
