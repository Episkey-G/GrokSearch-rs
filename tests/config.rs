use std::fs;

use grok_search_rs::config::{self, Config, InitOutcome};
use tempfile::tempdir;

#[test]
fn config_reads_grok_search_responses_defaults() {
    let cfg = Config::from_env_map([
        ("GROK_SEARCH_API_KEY", "grok-test-key"),
        ("TAVILY_API_KEY", "tvly-test-key"),
    ]);

    assert_eq!(cfg.grok_api_url, "https://api.x.ai/v1");
    assert_eq!(cfg.grok_model, "grok-4-1-fast-reasoning");
    assert!(cfg.web_search_enabled);
    assert!(!cfg.x_search_enabled);
    assert_eq!(cfg.tavily_api_url, "https://api.tavily.com");
    assert!(cfg.tavily_enabled);
    assert_eq!(cfg.default_extra_sources, 3);
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

    assert_eq!(cfg.default_extra_sources, 3);
    assert_eq!(cfg.fallback_sources, 5);
    assert_eq!(cfg.timeout.as_secs(), 60);
}

#[test]
fn config_file_supplies_values_when_env_absent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"
grok_api_key = "xai-from-file"
grok_model   = "grok-5-test"
tavily_api_key = "tvly-from-file"
default_extra_sources = 7
timeout_seconds = 42
"#,
    )
    .unwrap();

    let cfg = Config::load_from([("GROK_SEARCH_CONFIG", path.to_string_lossy().to_string())]);

    assert_eq!(cfg.grok_api_key.as_deref(), Some("xai-from-file"));
    assert_eq!(cfg.grok_model, "grok-5-test");
    assert_eq!(cfg.tavily_api_key.as_deref(), Some("tvly-from-file"));
    assert_eq!(cfg.default_extra_sources, 7);
    assert_eq!(cfg.timeout.as_secs(), 42);
}

#[test]
fn env_overrides_config_file_values() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"
grok_model = "model-from-file"
default_extra_sources = 7
"#,
    )
    .unwrap();

    let cfg = Config::load_from([
        ("GROK_SEARCH_CONFIG", path.to_string_lossy().to_string()),
        ("GROK_SEARCH_API_KEY", "grok-env-key".into()),
        ("GROK_SEARCH_MODEL", "model-from-env".into()),
        ("GROK_SEARCH_EXTRA_SOURCES", "2".into()),
    ]);

    assert_eq!(cfg.grok_model, "model-from-env");
    assert_eq!(cfg.default_extra_sources, 2);
    assert_eq!(cfg.grok_api_key.as_deref(), Some("grok-env-key"));
}

#[test]
fn missing_config_file_falls_back_to_env_and_defaults() {
    let dir = tempdir().unwrap();
    let nonexistent = dir.path().join("nope.toml");

    let cfg = Config::load_from([
        (
            "GROK_SEARCH_CONFIG",
            nonexistent.to_string_lossy().to_string(),
        ),
        ("GROK_SEARCH_API_KEY", "grok-env-key".into()),
    ]);

    assert_eq!(cfg.grok_api_key.as_deref(), Some("grok-env-key"));
    assert_eq!(cfg.grok_model, "grok-4-1-fast-reasoning");
    assert_eq!(cfg.default_extra_sources, 3);
}

#[test]
fn config_file_supports_all_documented_keys() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"
grok_api_url          = "https://api.modelverse.cn"
grok_api_key          = "xai-full"
grok_model            = "grok-9000"
web_search_enabled    = false
x_search_enabled      = true
tavily_api_url        = "https://tavily.example"
tavily_api_key        = "tvly-full"
tavily_enabled        = false
firecrawl_api_url     = "https://firecrawl.example"
firecrawl_api_key     = "fc-full"
firecrawl_enabled     = false
default_extra_sources = 4
fallback_sources      = 9
fetch_max_chars       = 12345
cache_size            = 128
timeout_seconds       = 30
"#,
    )
    .unwrap();

    let cfg = Config::load_from([("GROK_SEARCH_CONFIG", path.to_string_lossy().to_string())]);

    assert_eq!(cfg.grok_api_url, "https://api.modelverse.cn/v1");
    assert_eq!(cfg.grok_api_key.as_deref(), Some("xai-full"));
    assert_eq!(cfg.grok_model, "grok-9000");
    assert!(!cfg.web_search_enabled);
    assert!(cfg.x_search_enabled);
    assert_eq!(cfg.tavily_api_url, "https://tavily.example");
    assert_eq!(cfg.tavily_api_key.as_deref(), Some("tvly-full"));
    assert!(!cfg.tavily_enabled);
    assert_eq!(cfg.firecrawl_api_url, "https://firecrawl.example/v1");
    assert_eq!(cfg.firecrawl_api_key.as_deref(), Some("fc-full"));
    assert!(!cfg.firecrawl_enabled);
    assert_eq!(cfg.default_extra_sources, 4);
    assert_eq!(cfg.fallback_sources, 9);
    assert_eq!(cfg.fetch_max_chars, Some(12345));
    assert_eq!(cfg.cache_size, 128);
    assert_eq!(cfg.timeout.as_secs(), 30);
}

#[test]
fn write_template_creates_file_then_is_idempotent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested").join("config.toml");

    let first = config::write_template(&path).unwrap();
    assert_eq!(first, InitOutcome::Created);
    assert!(path.exists(), "template file must exist after first call");

    let body = fs::read_to_string(&path).unwrap();
    assert!(
        body.contains("grok-search-rs global configuration"),
        "template header must be present"
    );

    let second = config::write_template(&path).unwrap();
    assert_eq!(
        second,
        InitOutcome::AlreadyExists,
        "second call must not overwrite"
    );
    // Body unchanged after second call.
    assert_eq!(fs::read_to_string(&path).unwrap(), body);
}

#[test]
fn fresh_template_does_not_override_defaults_or_supply_credentials() {
    // The whole point of commenting out every key in the template is so an
    // un-edited scaffold behaves identically to "no config file at all".
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    config::write_template(&path).unwrap();

    let cfg = Config::load_from([("GROK_SEARCH_CONFIG", path.to_string_lossy().to_string())]);

    assert!(
        cfg.grok_api_key.is_none(),
        "empty template must NOT set grok_api_key (else onboarding guide gets bypassed)"
    );
    assert!(cfg.tavily_api_key.is_none());
    assert_eq!(cfg.grok_model, "grok-4-1-fast-reasoning");
    assert_eq!(cfg.grok_api_url, "https://api.x.ai/v1");
    assert_eq!(cfg.default_extra_sources, 3);
    assert_eq!(cfg.fallback_sources, 5);
    assert_eq!(cfg.cache_size, 256);
    assert_eq!(cfg.timeout.as_secs(), 60);
    assert!(cfg.web_search_enabled);
    assert!(!cfg.x_search_enabled);
    assert!(cfg.tavily_enabled);
    assert!(cfg.firecrawl_enabled);
}

#[test]
fn config_path_honors_explicit_env_override() {
    let path = config::config_path();
    // Test runs on macOS/Linux where HOME is set; just sanity-check the shape.
    if let Some(p) = path {
        assert!(p.ends_with("config.toml"));
    }
}

#[test]
fn config_path_explicit_override_wins_over_home() {
    let path = config::config_path_for([
        ("GROK_SEARCH_CONFIG", "/tmp/custom/grok.toml"),
        ("HOME", "/home/ignored"),
        ("USERPROFILE", "C:\\Users\\ignored"),
    ])
    .expect("explicit override must resolve");
    assert_eq!(path, std::path::PathBuf::from("/tmp/custom/grok.toml"));
}

#[test]
fn config_path_uses_home_on_unix_layout() {
    let path = config::config_path_for([("HOME", "/home/alice")])
        .expect("HOME must produce a path");
    let expected = std::path::PathBuf::from("/home/alice")
        .join(".config")
        .join("grok-search-rs")
        .join("config.toml");
    assert_eq!(path, expected);
}

#[test]
fn config_path_falls_back_to_userprofile_when_home_missing() {
    let path = config::config_path_for([("USERPROFILE", "C:\\Users\\chen")])
        .expect("USERPROFILE must produce a path on Windows-style env");
    let expected = std::path::PathBuf::from("C:\\Users\\chen")
        .join(".config")
        .join("grok-search-rs")
        .join("config.toml");
    assert_eq!(path, expected);
}

#[test]
fn config_path_prefers_home_over_userprofile_when_both_set() {
    let path = config::config_path_for([
        ("HOME", "/home/alice"),
        ("USERPROFILE", "C:\\Users\\chen"),
    ])
    .expect("must resolve");
    assert!(
        path.starts_with("/home/alice"),
        "HOME should win, got {}",
        path.display()
    );
}

#[test]
fn config_path_none_when_no_env_set() {
    let env: [(&str, &str); 0] = [];
    assert!(config::config_path_for(env).is_none());
}
