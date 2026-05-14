use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub provider: String,
    pub ai_api_url: String,
    pub ai_api_key: Option<String>,
    pub ai_model: String,
    pub web_search_enabled: bool,
    pub x_search_enabled: bool,
    pub tavily_api_url: String,
    pub tavily_api_key: Option<String>,
    pub tavily_enabled: bool,
    pub default_extra_sources: usize,
    pub fallback_sources: usize,
    pub cache_size: usize,
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

        let provider = get(&map, "GROK_SEARCH_RS_PROVIDER", "anthropic");

        Self {
            ai_api_url: ai_api_url(&map, &provider),
            ai_api_key: ai_api_key(&map, &provider),
            ai_model: ai_model(&map, &provider),
            provider,
            web_search_enabled: map
                .get("GROK_SEARCH_RS_WEB_SEARCH")
                .or_else(|| map.get("ANTHROPIC_WEB_SEARCH"))
                .or_else(|| map.get("OPENAI_WEB_SEARCH"))
                .or_else(|| map.get("XAI_WEB_SEARCH"))
                .map(|value| bool_literal(value))
                .unwrap_or(true),
            x_search_enabled: map
                .get("GROK_SEARCH_RS_X_SEARCH")
                .or_else(|| map.get("OPENAI_X_SEARCH"))
                .or_else(|| map.get("XAI_X_SEARCH"))
                .map(|value| bool_literal(value))
                .unwrap_or(false),
            tavily_api_url: get(&map, "TAVILY_API_URL", "https://api.tavily.com"),
            tavily_api_key: map.get("TAVILY_API_KEY").cloned(),
            tavily_enabled: bool_value(&map, "TAVILY_ENABLED", true),
            default_extra_sources: usize_value(&map, "GROK_SEARCH_RS_EXTRA_SOURCES", 0),
            fallback_sources: usize_value(&map, "GROK_SEARCH_RS_FALLBACK_SOURCES", 5),
            cache_size: usize_value(&map, "GROK_SEARCH_RS_CACHE_SIZE", 256),
        }
    }

    pub fn redacted_diagnostics(&self) -> String {
        format!(
            "provider={} ai_api_url={} ai_api_key={} ai_model={} web_search_enabled={} x_search_enabled={} tavily_api_key={} default_extra_sources={} fallback_sources={}",
            self.provider,
            self.ai_api_url,
            redact(self.ai_api_key.as_deref()),
            self.ai_model,
            self.web_search_enabled,
            self.x_search_enabled,
            redact(self.tavily_api_key.as_deref()),
            self.default_extra_sources,
            self.fallback_sources
        )
    }
}

fn get(map: &HashMap<String, String>, key: &str, default: &str) -> String {
    map.get(key).cloned().unwrap_or_else(|| default.to_string())
}

fn ai_api_url(map: &HashMap<String, String>, provider: &str) -> String {
    match provider {
        "anthropic" => map
            .get("ANTHROPIC_API_URL")
            .or_else(|| map.get("GROK_API_URL"))
            .or_else(|| map.get("OPENAI_API_URL"))
            .or_else(|| map.get("XAI_API_URL"))
            .cloned()
            .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
        _ => map
            .get("OPENAI_API_URL")
            .or_else(|| map.get("XAI_API_URL"))
            .or_else(|| map.get("GROK_API_URL"))
            .or_else(|| map.get("ANTHROPIC_API_URL"))
            .cloned()
            .unwrap_or_else(|| "https://api.x.ai/v1".to_string()),
    }
}

fn ai_api_key(map: &HashMap<String, String>, provider: &str) -> Option<String> {
    match provider {
        "anthropic" => map
            .get("ANTHROPIC_API_KEY")
            .or_else(|| map.get("GROK_API_KEY"))
            .or_else(|| map.get("OPENAI_API_KEY"))
            .or_else(|| map.get("XAI_API_KEY"))
            .cloned(),
        _ => map
            .get("OPENAI_API_KEY")
            .or_else(|| map.get("XAI_API_KEY"))
            .or_else(|| map.get("GROK_API_KEY"))
            .or_else(|| map.get("ANTHROPIC_API_KEY"))
            .cloned(),
    }
}

fn ai_model(map: &HashMap<String, String>, provider: &str) -> String {
    match provider {
        "anthropic" => map
            .get("ANTHROPIC_MODEL")
            .or_else(|| map.get("GROK_MODEL"))
            .or_else(|| map.get("OPENAI_MODEL"))
            .or_else(|| map.get("XAI_MODEL"))
            .cloned()
            .unwrap_or_else(|| "grok-4-1-fast-reasoning".to_string()),
        _ => map
            .get("OPENAI_MODEL")
            .or_else(|| map.get("XAI_MODEL"))
            .or_else(|| map.get("GROK_MODEL"))
            .or_else(|| map.get("ANTHROPIC_MODEL"))
            .cloned()
            .unwrap_or_else(|| "grok-4.3".to_string()),
    }
}

fn bool_value(map: &HashMap<String, String>, key: &str, default: bool) -> bool {
    map.get(key).map(|v| bool_literal(v)).unwrap_or(default)
}

fn bool_literal(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
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
