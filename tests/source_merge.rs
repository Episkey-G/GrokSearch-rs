use grok_search_rs::model::source::{merge_sources, Source};

#[test]
fn merge_sources_dedupes_by_url_and_preserves_first_provider() {
    let xai = Source::new("https://openai.com/news", "xai_web_search").with_title("OpenAI News");
    let tavily = Source::new("https://openai.com/news", "tavily").with_title("Duplicate");
    let other = Source::new("https://example.com/a", "tavily");

    let merged = merge_sources(vec![xai], vec![tavily, other]);

    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].provider, "xai_web_search");
    assert_eq!(merged[0].title.as_deref(), Some("OpenAI News"));
    assert_eq!(merged[1].url, "https://example.com/a");
}
