use grok_search_rs::config::Config;

#[test]
fn config_reads_grok_search_responses_defaults() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
    ]);

    assert_eq!(cfg.grok_api_url, "https://api.x.ai/v1");
    assert_eq!(cfg.grok_model, "grok-4.3");
    assert!(cfg.web_search_enabled);
    assert!(!cfg.x_search_enabled);
    assert_eq!(cfg.tavily_api_url, "https://api.tavily.com");
    assert!(cfg.tavily_enabled);
    assert_eq!(cfg.default_extra_sources, 0);
    assert_eq!(cfg.fallback_sources, 5);
    assert_eq!(cfg.timeout.as_secs(), 60);
}

#[test]
fn config_normalizes_grok_search_url_to_v1_base() {
    let cases = [
        ("https://api.modelverse.cn", "https://api.modelverse.cn/v1"),
        ("https://api.modelverse.cn/", "https://api.modelverse.cn/v1"),
        (
            "https://api.modelverse.cn/v1",
            "https://api.modelverse.cn/v1",
        ),
        (
            "https://api.modelverse.cn/v1/responses",
            "https://api.modelverse.cn/v1",
        ),
    ];

    for (input, expected) in cases {
        let cfg = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "grok-test-key"),
            ("GROK_SEARCH_URL", input),
        ]);
        assert_eq!(cfg.grok_api_url, expected);
    }
}

#[test]
fn config_enables_x_search_only_when_configured() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-test-key"),
        ("GROK_SEARCH_X_SEARCH", "true"),
    ]);

    assert!(cfg.x_search_enabled);
}

#[test]
fn config_reads_firecrawl_settings() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-test-key"),
        ("FIRECRAWL_API_KEY", "fc-test-key"),
        ("FIRECRAWL_API_URL", "https://firecrawl.example/v1"),
        ("FIRECRAWL_ENABLED", "true"),
    ]);

    assert_eq!(cfg.firecrawl_api_url, "https://firecrawl.example/v1");
    assert_eq!(cfg.firecrawl_api_key.as_deref(), Some("fc-test-key"));
    assert!(cfg.firecrawl_enabled);
}

#[test]
fn config_redacts_grok_tavily_and_firecrawl_keys() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-1234567890"),
        ("TAVILY_API_KEY", "tvly-abcdefghi"),
        ("FIRECRAWL_API_KEY", "fc-abcdefghi"),
    ]);

    let info = cfg.redacted_diagnostics();
    assert!(info.contains("grok"));
    assert!(info.contains("tvly"));
    assert!(info.contains("fc-a"));
    assert!(!info.contains("1234567890"));
    assert!(!info.contains("abcdefghi"));
}

#[test]
fn config_reads_extra_sources_and_fallback_sources_from_env() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-test-key"),
        ("GROK_SEARCH_EXTRA_SOURCES", "3"),
        ("GROK_SEARCH_FALLBACK_SOURCES", "7"),
    ]);

    assert_eq!(cfg.default_extra_sources, 3);
    assert_eq!(cfg.fallback_sources, 7);
}

#[test]
fn config_reads_timeout_seconds() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-test-key"),
        ("GROK_SEARCH_TIMEOUT_SECONDS", "90"),
    ]);

    assert_eq!(cfg.timeout.as_secs(), 90);
}

#[test]
fn invalid_source_counts_fall_back_to_safe_defaults() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-test-key"),
        ("GROK_SEARCH_EXTRA_SOURCES", "not-a-number"),
        ("GROK_SEARCH_FALLBACK_SOURCES", "not-a-number"),
    ]);

    assert_eq!(cfg.default_extra_sources, 0);
    assert_eq!(cfg.fallback_sources, 5);
    assert_eq!(cfg.timeout.as_secs(), 60);
}
