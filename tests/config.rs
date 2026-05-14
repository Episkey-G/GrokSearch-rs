use grok_search_rs::config::Config;

#[test]
fn config_defaults_to_anthropic_provider() {
    let cfg = Config::from_env_map([
        ("ANTHROPIC_API_KEY", "anthropic-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
    ]);

    assert_eq!(cfg.provider, "anthropic");
    assert_eq!(cfg.ai_api_url, "https://api.anthropic.com/v1");
    assert_eq!(cfg.ai_model, "grok-4-1-fast-reasoning");
    assert!(cfg.web_search_enabled);
    assert!(!cfg.x_search_enabled);
}

#[test]
fn config_keeps_openai_responses_provider() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_RS_PROVIDER", "openai"),
        ("OPENAI_API_KEY", "openai-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
    ]);

    assert_eq!(cfg.provider, "openai");
    assert_eq!(cfg.ai_api_url, "https://api.x.ai/v1");
    assert_eq!(cfg.ai_model, "grok-4.3");
    assert!(cfg.web_search_enabled);
    assert!(!cfg.x_search_enabled);
}

#[test]
fn config_enables_x_search_only_when_configured() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_RS_PROVIDER", "openai"),
        ("GROK_SEARCH_RS_X_SEARCH", "true"),
        ("OPENAI_API_KEY", "openai-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
    ]);

    assert!(cfg.x_search_enabled);
}

#[test]
fn config_supports_anthropic_messages_compatibility_mode() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_RS_PROVIDER", "anthropic"),
        ("ANTHROPIC_API_KEY", "anthropic-test-key"),
        ("ANTHROPIC_API_URL", "http://64.186.226.237:8317/v1"),
        ("ANTHROPIC_MODEL", "grok-4-1-fast-reasoning"),
        ("TAVILY_API_KEY", "tvly-test-key"),
    ]);

    assert_eq!(cfg.provider, "anthropic");
    assert_eq!(cfg.ai_api_url, "http://64.186.226.237:8317/v1");
    assert_eq!(cfg.ai_model, "grok-4-1-fast-reasoning");
    assert!(cfg.web_search_enabled);
}

#[test]
fn config_redacts_secret_values() {
    let cfg = Config::from_env_map([
        ("ANTHROPIC_API_KEY", "anthropic-1234567890"),
        ("TAVILY_API_KEY", "tvly-abcdefghi"),
    ]);

    let info = cfg.redacted_diagnostics();
    assert!(info.contains("anth"));
    assert!(info.contains("***"));
    assert!(!info.contains("1234567890"));
    assert!(!info.contains("abcdefghi"));
}

#[test]
fn config_defaults_extra_sources_and_fallback_sources() {
    let cfg = Config::from_env_map([
        ("ANTHROPIC_API_KEY", "anthropic-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
    ]);

    assert_eq!(cfg.default_extra_sources, 0);
    assert_eq!(cfg.fallback_sources, 5);
}

#[test]
fn config_reads_extra_sources_and_fallback_sources_from_env() {
    let cfg = Config::from_env_map([
        ("ANTHROPIC_API_KEY", "anthropic-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
        ("GROK_SEARCH_RS_EXTRA_SOURCES", "3"),
        ("GROK_SEARCH_RS_FALLBACK_SOURCES", "7"),
    ]);

    assert_eq!(cfg.default_extra_sources, 3);
    assert_eq!(cfg.fallback_sources, 7);
}

#[test]
fn invalid_source_counts_fall_back_to_safe_defaults() {
    let cfg = Config::from_env_map([
        ("ANTHROPIC_API_KEY", "anthropic-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
        ("GROK_SEARCH_RS_EXTRA_SOURCES", "not-a-number"),
        ("GROK_SEARCH_RS_FALLBACK_SOURCES", "not-a-number"),
    ]);

    assert_eq!(cfg.default_extra_sources, 0);
    assert_eq!(cfg.fallback_sources, 5);
}
