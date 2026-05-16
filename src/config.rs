use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Responses,
    ChatCompletions,
}

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
    pub fetch_max_chars: Option<usize>,
    pub cache_size: usize,
    pub timeout: Duration,
    pub openai_compatible_api_url: Option<String>,
    pub openai_compatible_api_key: Option<String>,
    pub openai_compatible_model: Option<String>,
    pub transport: Transport,
}

/// Mirror of `Config` for TOML deserialization. All fields optional so users
/// only need to set what they care about. Field names map 1:1 to TOML keys.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct ConfigFile {
    grok_api_url: Option<String>,
    grok_api_key: Option<String>,
    grok_model: Option<String>,
    web_search_enabled: Option<bool>,
    x_search_enabled: Option<bool>,
    tavily_api_url: Option<String>,
    tavily_api_key: Option<String>,
    tavily_enabled: Option<bool>,
    firecrawl_api_url: Option<String>,
    firecrawl_api_key: Option<String>,
    firecrawl_enabled: Option<bool>,
    default_extra_sources: Option<usize>,
    fallback_sources: Option<usize>,
    fetch_max_chars: Option<usize>,
    cache_size: Option<usize>,
    timeout_seconds: Option<u64>,
    openai_compatible_api_url: Option<String>,
    openai_compatible_api_key: Option<String>,
    openai_compatible_model: Option<String>,
}

impl ConfigFile {
    /// Translate file fields into the env-style key/value map the rest of the
    /// loader consumes. Keeps a single precedence pipeline.
    fn into_env_map(self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let mut insert = |key: &str, value: Option<String>| {
            if let Some(v) = value {
                out.insert(key.to_string(), v);
            }
        };
        insert("GROK_SEARCH_URL", self.grok_api_url);
        insert("GROK_SEARCH_API_KEY", self.grok_api_key);
        insert("GROK_SEARCH_MODEL", self.grok_model);
        insert(
            "GROK_SEARCH_WEB_SEARCH",
            self.web_search_enabled.map(|b| b.to_string()),
        );
        insert(
            "GROK_SEARCH_X_SEARCH",
            self.x_search_enabled.map(|b| b.to_string()),
        );
        insert("TAVILY_API_URL", self.tavily_api_url);
        insert("TAVILY_API_KEY", self.tavily_api_key);
        insert("TAVILY_ENABLED", self.tavily_enabled.map(|b| b.to_string()));
        insert("FIRECRAWL_API_URL", self.firecrawl_api_url);
        insert("FIRECRAWL_API_KEY", self.firecrawl_api_key);
        insert(
            "FIRECRAWL_ENABLED",
            self.firecrawl_enabled.map(|b| b.to_string()),
        );
        insert(
            "GROK_SEARCH_EXTRA_SOURCES",
            self.default_extra_sources.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_FALLBACK_SOURCES",
            self.fallback_sources.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_FETCH_MAX_CHARS",
            self.fetch_max_chars.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_CACHE_SIZE",
            self.cache_size.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_TIMEOUT_SECONDS",
            self.timeout_seconds.map(|n| n.to_string()),
        );
        insert("OPENAI_COMPATIBLE_API_URL", self.openai_compatible_api_url);
        insert("OPENAI_COMPATIBLE_API_KEY", self.openai_compatible_api_key);
        insert("OPENAI_COMPATIBLE_MODEL", self.openai_compatible_model);
        out
    }
}

impl Config {
    /// Load config with full precedence chain: process env > config file > defaults.
    /// Config file path: `$GROK_SEARCH_CONFIG` if set, else `~/.config/grok-search-rs/config.toml`.
    /// Missing or unparseable files are skipped silently (env-only mode).
    pub fn load() -> Self {
        Self::load_from(std::env::vars())
    }

    /// Same as `load`, but uses a caller-supplied env map. Lets tests exercise
    /// the file + env merge without mutating process-global env state.
    pub fn load_from<I, K, V>(env_vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env_map: HashMap<String, String> = env_vars
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        let file_map = resolve_config_path(&env_map)
            .and_then(|path| load_file_map(&path))
            .unwrap_or_default();
        Self::from_env_map(merge_env_over_file(file_map, env_map))
    }

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
            grok_model: get(&map, "GROK_SEARCH_MODEL", "grok-4-1-fast-reasoning"),
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
            default_extra_sources: usize_value(&map, "GROK_SEARCH_EXTRA_SOURCES", 3),
            fallback_sources: usize_value(&map, "GROK_SEARCH_FALLBACK_SOURCES", 5),
            fetch_max_chars: optional_positive_usize(&map, "GROK_SEARCH_FETCH_MAX_CHARS"),
            cache_size: usize_value(&map, "GROK_SEARCH_CACHE_SIZE", 256),
            timeout: Duration::from_secs(u64_value(&map, "GROK_SEARCH_TIMEOUT_SECONDS", 60)),
            openai_compatible_api_url: map
                .get("OPENAI_COMPATIBLE_API_URL")
                .cloned()
                .filter(|v| !v.is_empty()),
            openai_compatible_api_key: map
                .get("OPENAI_COMPATIBLE_API_KEY")
                .cloned()
                .filter(|v| !v.is_empty()),
            openai_compatible_model: map
                .get("OPENAI_COMPATIBLE_MODEL")
                .cloned()
                .filter(|v| !v.is_empty()),
            transport: decide_transport(&map),
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

fn resolve_config_path(env: &HashMap<String, String>) -> Option<PathBuf> {
    if let Some(explicit) = env.get("GROK_SEARCH_CONFIG").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(explicit));
    }
    let home = env.get("HOME").filter(|v| !v.is_empty())?;
    Some(
        Path::new(home)
            .join(".config")
            .join("grok-search-rs")
            .join("config.toml"),
    )
}

/// Resolved config file path using process env (`$GROK_SEARCH_CONFIG` or
/// `~/.config/grok-search-rs/config.toml`). `None` when `HOME` is unset and no
/// explicit path is provided.
pub fn config_path() -> Option<PathBuf> {
    let env: HashMap<String, String> = std::env::vars().collect();
    resolve_config_path(&env)
}

/// Outcome of a `--init` scaffold attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitOutcome {
    Created,
    AlreadyExists,
}

/// Idempotent template writer used by `grok-search-rs --init`. Returns
/// `AlreadyExists` without touching the file when it already exists; otherwise
/// creates parent dirs and writes the annotated template (all keys commented).
pub fn write_template(path: &Path) -> std::io::Result<InitOutcome> {
    if path.exists() {
        return Ok(InitOutcome::AlreadyExists);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, CONFIG_TEMPLATE)?;
    Ok(InitOutcome::Created)
}

/// Embedded TOML template. All keys are commented so an empty scaffold cannot
/// silently override built-in defaults; the user uncomments only what they need.
pub const CONFIG_TEMPLATE: &str = r#"# grok-search-rs global configuration
# Path: ~/.config/grok-search-rs/config.toml  (or $GROK_SEARCH_CONFIG)
#
# Precedence: process env > this file > built-in defaults.
# All keys below are commented out; uncomment and fill what you need.
# Unknown keys are rejected — typos surface as errors, not silent drops.

# ── Required ──────────────────────────────────────────────────
# grok_api_key   = "xai-..."          # xAI / Grok key   https://x.ai/api
# tavily_api_key = "tvly-..."         # Tavily key       https://tavily.com

# ── Common knobs ──────────────────────────────────────────────
# grok_model         = "grok-4-1-fast-reasoning"
# x_search_enabled   = false          # Grok X/Twitter search tool
# firecrawl_api_key  = "fc-..."       # Optional fetch fallback   https://firecrawl.dev

# ── Endpoints (only set when using a self-hosted gateway) ─────
# grok_api_url      = "https://api.x.ai"
# tavily_api_url    = "https://api.tavily.com"
# firecrawl_api_url = "https://api.firecrawl.dev"

# ── OpenAI-compatible transport (alternative to grok_*) ───────
# Set these three to use a /v1/chat/completions gateway. When grok_api_key
# above is also set, it wins; otherwise these three pick the chat-completions
# transport. Source extraction supports OpenAI annotations, Perplexity-style
# citations, marybrown's top-level search_sources, and inline [[n]](url).
# openai_compatible_api_url = "https://your-gateway/v1"
# openai_compatible_api_key = "sk-..."
# openai_compatible_model   = "grok-4.3-fast"

# ── Feature toggles ───────────────────────────────────────────
# web_search_enabled = true
# tavily_enabled     = true
# firecrawl_enabled  = true

# ── Behavior tuning ───────────────────────────────────────────
# default_extra_sources = 3
# fallback_sources      = 5
# fetch_max_chars       = 200000      # per-request char cap on web_fetch
# cache_size            = 256
# timeout_seconds       = 60
"#;

fn load_file_map(path: &Path) -> Option<HashMap<String, String>> {
    let body = std::fs::read_to_string(path).ok()?;
    match toml::from_str::<ConfigFile>(&body) {
        Ok(file) => Some(file.into_env_map()),
        Err(err) => {
            eprintln!(
                "grok-search-rs: ignoring malformed config {}: {}",
                path.display(),
                err
            );
            None
        }
    }
}

fn merge_env_over_file(
    mut base: HashMap<String, String>,
    overlay: HashMap<String, String>,
) -> HashMap<String, String> {
    for (k, v) in overlay {
        base.insert(k, v);
    }
    base
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

fn optional_positive_usize(map: &HashMap<String, String>, key: &str) -> Option<usize> {
    map.get(key)
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
}

fn decide_transport(map: &HashMap<String, String>) -> Transport {
    let grok_key_set = map
        .get("GROK_SEARCH_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let compat_url_set = map
        .get("OPENAI_COMPATIBLE_API_URL")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let compat_key_set = map
        .get("OPENAI_COMPATIBLE_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    if grok_key_set {
        return Transport::Responses;
    }
    if compat_url_set && compat_key_set {
        return Transport::ChatCompletions;
    }
    Transport::Responses
}

fn redact(value: Option<&str>) -> String {
    match value {
        None => "unset".to_string(),
        Some(v) if v.len() <= 8 => "***".to_string(),
        Some(v) => format!("{}***{}", &v[..4], &v[v.len() - 4..]),
    }
}

#[cfg(test)]
mod transport_field_tests {
    use super::*;

    #[test]
    fn loads_openai_compatible_fields_from_env() {
        let cfg = Config::from_env_map([
            ("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
            ("OPENAI_COMPATIBLE_MODEL", "grok-4.3-fast"),
        ]);
        assert_eq!(
            cfg.openai_compatible_api_url.as_deref(),
            Some("https://example.com/v1")
        );
        assert_eq!(cfg.openai_compatible_api_key.as_deref(), Some("sk-fake"));
        assert_eq!(cfg.openai_compatible_model.as_deref(), Some("grok-4.3-fast"));
    }

    #[test]
    fn transport_defaults_to_responses_when_only_grok_set() {
        let cfg = Config::from_env_map([("GROK_SEARCH_API_KEY", "xai-fake")]);
        assert_eq!(cfg.transport, Transport::Responses);
    }

    #[test]
    fn transport_chat_completions_when_only_compat_set() {
        let cfg = Config::from_env_map([
            ("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
        ]);
        assert_eq!(cfg.transport, Transport::ChatCompletions);
    }

    #[test]
    fn transport_prefers_grok_when_both_set() {
        let cfg = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "xai-fake"),
            ("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
        ]);
        assert_eq!(cfg.transport, Transport::Responses);
    }
}
