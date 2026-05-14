use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub grok_api_url: String,
    pub grok_api_key: Option<String>,
    pub grok_model: String,
    pub web_search_enabled: bool,
    pub x_search_enabled: bool,
    pub tavily_api_url: String,
    pub tavily_api_key: Option<String>,
    pub tavily_enabled: bool,
    pub firecrawl_api_url: String,
    pub firecrawl_api_key: Option<String>,
    pub firecrawl_enabled: bool,
    pub default_extra_sources: usize,
    pub fallback_sources: usize,
    pub cache_size: usize,
    pub timeout: Duration,
}

impl Config {
    pub fn from_env() -> Self {
        Self::from_env_map(std::env::vars())
    }

    pub fn from_env_map<I, K, V>(vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let map: HashMap<String, String> = vars
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();

        Self {
            grok_api_url: normalize_v1_base(&get(&map, "GROK_SEARCH_URL", "https://api.x.ai")),
            grok_api_key: map.get("GROK_SEARCH_API_KEY").cloned(),
            grok_model: get(&map, "GROK_SEARCH_MODEL", "grok-4.3"),
            web_search_enabled: bool_value(&map, "GROK_SEARCH_WEB_SEARCH", true),
            x_search_enabled: bool_value(&map, "GROK_SEARCH_X_SEARCH", false),
            tavily_api_url: normalize_plain_base(&get(
                &map,
                "TAVILY_API_URL",
                "https://api.tavily.com",
            )),
            tavily_api_key: map.get("TAVILY_API_KEY").cloned(),
            tavily_enabled: bool_value(&map, "TAVILY_ENABLED", true),
            firecrawl_api_url: normalize_v1_base(&get(
                &map,
                "FIRECRAWL_API_URL",
                "https://api.firecrawl.dev",
            )),
            firecrawl_api_key: map.get("FIRECRAWL_API_KEY").cloned(),
            firecrawl_enabled: bool_value(&map, "FIRECRAWL_ENABLED", true),
            default_extra_sources: usize_value(&map, "GROK_SEARCH_EXTRA_SOURCES", 0),
            fallback_sources: usize_value(&map, "GROK_SEARCH_FALLBACK_SOURCES", 5),
            cache_size: usize_value(&map, "GROK_SEARCH_CACHE_SIZE", 256),
            timeout: Duration::from_secs(u64_value(&map, "GROK_SEARCH_TIMEOUT_SECONDS", 60)),
        }
    }

    pub fn redacted_diagnostics(&self) -> String {
        format!(
            "grok_api_url={} grok_api_key={} grok_model={} web_search_enabled={} x_search_enabled={} tavily_api_key={} firecrawl_api_key={} default_extra_sources={} fallback_sources={} timeout_seconds={}",
            self.grok_api_url,
            redact(self.grok_api_key.as_deref()),
            self.grok_model,
            self.web_search_enabled,
            self.x_search_enabled,
            redact(self.tavily_api_key.as_deref()),
            redact(self.firecrawl_api_key.as_deref()),
            self.default_extra_sources,
            self.fallback_sources,
            self.timeout.as_secs()
        )
    }
}

fn get(map: &HashMap<String, String>, key: &str, default: &str) -> String {
    map.get(key).cloned().unwrap_or_else(|| default.to_string())
}

pub fn normalize_v1_base(url: &str) -> String {
    let mut value = url.trim().trim_end_matches('/').to_string();
    for suffix in ["/responses"] {
        if value.ends_with(suffix) {
            let keep = value.len() - suffix.len();
            value.truncate(keep);
            value = value.trim_end_matches('/').to_string();
        }
    }
    if !value.ends_with("/v1") {
        value.push_str("/v1");
    }
    value
}

fn normalize_plain_base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn bool_value(map: &HashMap<String, String>, key: &str, default: bool) -> bool {
    map.get(key).map(|v| bool_literal(v)).unwrap_or(default)
}

fn bool_literal(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

fn u64_value(map: &HashMap<String, String>, key: &str, default: u64) -> u64 {
    map.get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn usize_value(map: &HashMap<String, String>, key: &str, default: usize) -> usize {
    map.get(key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn redact(value: Option<&str>) -> String {
    match value {
        None => "unset".to_string(),
        Some(v) if v.len() <= 8 => "***".to_string(),
        Some(v) => format!("{}***{}", &v[..4], &v[v.len() - 4..]),
    }
}
