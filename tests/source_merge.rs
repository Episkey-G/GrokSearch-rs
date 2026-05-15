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
