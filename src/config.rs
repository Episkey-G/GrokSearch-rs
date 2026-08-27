use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Responses,
    ChatCompletions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    ApiKey,
    OAuth,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    pub grok_api_url: String,
    pub grok_api_key: Option<String>,
    pub grok_auth_mode: AuthMode,
    pub grok_auth_file: Option<PathBuf>,
    pub grok_model: String,
    pub web_search_enabled: bool,
    pub x_search_enabled: bool,
    pub tavily_api_url: String,
    pub tavily_api_key: Option<String>,
    pub tavily_enabled: bool,
    pub firecrawl_api_url: String,
    pub firecrawl_api_key: Option<String>,
    pub firecrawl_enabled: bool,
    pub tinyfish_search_api_url: String,
    pub tinyfish_fetch_api_url: String,
    pub tinyfish_api_key: Option<String>,
    pub tinyfish_enabled: bool,
    pub exa_api_url: String,
    pub exa_api_key: Option<String>,
    pub exa_enabled: bool,
    /// Explicit source-provider chain order (lowercased names). Empty means
    /// "use the built-in canonical order over whatever is configured".
    pub source_providers: Vec<String>,
    pub default_extra_sources: usize,
    pub fallback_sources: usize,
    pub fetch_max_chars: Option<usize>,
    pub cache_size: usize,
    pub timeout: Duration,
    pub openai_compatible_api_url: Option<String>,
    pub openai_compatible_api_key: Option<String>,
    pub openai_compatible_model: Option<String>,
    pub transport: Transport,
    pub github_token: Option<String>,
    pub source_max_answers: usize,
    pub source_max_comments: usize,
    pub enrich_concurrency: usize,
    pub enrich_max_chars: usize,
    pub max_inline_sources: usize,
    pub response_max_chars: usize,
    /// Where the config file was looked for, whether or not one was there.
    /// Worth reporting even when absent — it tells the operator where to put
    /// one.
    pub config_file_path: Option<PathBuf>,
    /// What became of that file. Rejection is deliberate — one unknown key
    /// voids the whole file rather than half-applying it — but the only sign
    /// of it used to be a line on stderr, which an MCP client swallows,
    /// leaving the operator staring at defaults with no way to learn why.
    pub config_file_state: ConfigFileState,
}

/// The outcome of loading the config file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConfigFileState {
    /// Nothing at the resolved path, or no path could be resolved. Not a
    /// failure: running purely on environment variables is normal.
    #[default]
    Absent,
    Loaded,
    /// Present but unusable, with the reason attached verbatim so the operator
    /// sees which key or line is at fault.
    Rejected(String),
}

impl ConfigFileState {
    /// Stable label for diagnostics output.
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfigFileState::Absent => "absent",
            ConfigFileState::Loaded => "loaded",
            ConfigFileState::Rejected(_) => "rejected",
        }
    }

    /// The rejection reason, if this is a rejection.
    pub fn detail(&self) -> Option<&str> {
        match self {
            ConfigFileState::Rejected(detail) => Some(detail),
            _ => None,
        }
    }
}

/// Hand-written `Debug` that masks secret-bearing fields so a stray
/// `{:?}`/`{:#?}` of a `Config` can never leak credentials. Secret `Option`
/// fields render as a two-state `"set"`/`"unset"` marker (mirroring
/// [`Config::github_token_status`]); every non-secret field stays readable.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn mask<T>(value: &Option<T>) -> &'static str {
            if value.is_some() {
                "set"
            } else {
                "unset"
            }
        }
        f.debug_struct("Config")
            .field("grok_api_url", &self.grok_api_url)
            .field("grok_api_key", &mask(&self.grok_api_key))
            .field("grok_auth_mode", &self.grok_auth_mode)
            .field("grok_auth_file", &self.grok_auth_file)
            .field("grok_model", &self.grok_model)
            .field("web_search_enabled", &self.web_search_enabled)
            .field("x_search_enabled", &self.x_search_enabled)
            .field("tavily_api_url", &self.tavily_api_url)
            .field("tavily_api_key", &mask(&self.tavily_api_key))
            .field("tavily_enabled", &self.tavily_enabled)
            .field("firecrawl_api_url", &self.firecrawl_api_url)
            .field("firecrawl_api_key", &mask(&self.firecrawl_api_key))
            .field("firecrawl_enabled", &self.firecrawl_enabled)
            .field("tinyfish_search_api_url", &self.tinyfish_search_api_url)
            .field("tinyfish_fetch_api_url", &self.tinyfish_fetch_api_url)
            .field("tinyfish_api_key", &mask(&self.tinyfish_api_key))
            .field("tinyfish_enabled", &self.tinyfish_enabled)
            .field("exa_api_url", &self.exa_api_url)
            .field("exa_api_key", &mask(&self.exa_api_key))
            .field("exa_enabled", &self.exa_enabled)
            .field("source_providers", &self.source_providers)
            .field("default_extra_sources", &self.default_extra_sources)
            .field("fallback_sources", &self.fallback_sources)
            .field("fetch_max_chars", &self.fetch_max_chars)
            .field("cache_size", &self.cache_size)
            .field("timeout", &self.timeout)
            .field("openai_compatible_api_url", &self.openai_compatible_api_url)
            .field(
                "openai_compatible_api_key",
                &mask(&self.openai_compatible_api_key),
            )
            .field("openai_compatible_model", &self.openai_compatible_model)
            .field("transport", &self.transport)
            .field("github_token", &mask(&self.github_token))
            .field("source_max_answers", &self.source_max_answers)
            .field("source_max_comments", &self.source_max_comments)
            .field("enrich_concurrency", &self.enrich_concurrency)
            .field("enrich_max_chars", &self.enrich_max_chars)
            .field("max_inline_sources", &self.max_inline_sources)
            .field("response_max_chars", &self.response_max_chars)
            .field("config_file_path", &self.config_file_path)
            .field("config_file_state", &self.config_file_state)
            .finish()
    }
}

/// Mirror of `Config` for TOML deserialization. All fields optional so users
/// only need to set what they care about. Field names map 1:1 to TOML keys.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct ConfigFile {
    grok_api_url: Option<String>,
    grok_api_key: Option<String>,
    grok_auth_mode: Option<String>,
    grok_auth_file: Option<String>,
    grok_model: Option<String>,
    web_search_enabled: Option<bool>,
    x_search_enabled: Option<bool>,
    tavily_api_url: Option<String>,
    tavily_api_key: Option<String>,
    tavily_enabled: Option<bool>,
    firecrawl_api_url: Option<String>,
    firecrawl_api_key: Option<String>,
    firecrawl_enabled: Option<bool>,
    tinyfish_search_api_url: Option<String>,
    tinyfish_fetch_api_url: Option<String>,
    tinyfish_api_key: Option<String>,
    tinyfish_enabled: Option<bool>,
    exa_api_url: Option<String>,
    exa_api_key: Option<String>,
    exa_enabled: Option<bool>,
    source_providers: Option<Vec<String>>,
    default_extra_sources: Option<usize>,
    fallback_sources: Option<usize>,
    fetch_max_chars: Option<usize>,
    cache_size: Option<usize>,
    timeout_seconds: Option<u64>,
    openai_compatible_api_url: Option<String>,
    openai_compatible_api_key: Option<String>,
    openai_compatible_model: Option<String>,
    github_token: Option<String>,
    source_max_answers: Option<usize>,
    source_max_comments: Option<usize>,
    enrich_concurrency: Option<usize>,
    enrich_max_chars: Option<usize>,
    max_inline_sources: Option<usize>,
    response_max_chars: Option<usize>,
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
        insert("GROK_SEARCH_AUTH_MODE", self.grok_auth_mode);
        insert("GROK_SEARCH_AUTH_FILE", self.grok_auth_file);
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
        insert("TINYFISH_SEARCH_API_URL", self.tinyfish_search_api_url);
        insert("TINYFISH_FETCH_API_URL", self.tinyfish_fetch_api_url);
        insert("TINYFISH_API_KEY", self.tinyfish_api_key);
        insert(
            "TINYFISH_ENABLED",
            self.tinyfish_enabled.map(|b| b.to_string()),
        );
        insert("EXA_API_URL", self.exa_api_url);
        insert("EXA_API_KEY", self.exa_api_key);
        insert("EXA_ENABLED", self.exa_enabled.map(|b| b.to_string()));
        insert(
            "GROK_SEARCH_SOURCE_PROVIDERS",
            self.source_providers.map(|list| list.join(",")),
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
        insert("GITHUB_TOKEN", self.github_token);
        insert(
            "GROK_SEARCH_SOURCE_MAX_ANSWERS",
            self.source_max_answers.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_SOURCE_MAX_COMMENTS",
            self.source_max_comments.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_ENRICH_CONCURRENCY",
            self.enrich_concurrency.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_ENRICH_MAX_CHARS",
            self.enrich_max_chars.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_MAX_INLINE_SOURCES",
            self.max_inline_sources.map(|n| n.to_string()),
        );
        insert(
            "GROK_SEARCH_RESPONSE_MAX_CHARS",
            self.response_max_chars.map(|n| n.to_string()),
        );
        out
    }
}

impl Config {
    /// Load config with full precedence chain: process env > config file > defaults.
    /// Config file path: `$GROK_SEARCH_CONFIG` if set, else
    /// `<home>/.config/grok-search-rs/config.toml`, where `<home>` is `$HOME`
    /// on Unix / Git Bash and `%USERPROFILE%` on native Windows shells.
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
        let path = resolve_config_path(&env_map);
        let (file_map, config_file_state) = match path.as_deref() {
            Some(path) => read_config_file(path),
            None => (HashMap::new(), ConfigFileState::Absent),
        };
        let mut config = Self::from_env_map(merge_env_over_file(file_map, env_map));
        config.config_file_path = path;
        config.config_file_state = config_file_state;
        config
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
        let grok_auth_mode = auth_mode_value(&map);

        Self {
            grok_api_url: normalize_v1_base(&get(&map, "GROK_SEARCH_URL", "https://api.x.ai")),
            grok_api_key: map.get("GROK_SEARCH_API_KEY").cloned(),
            grok_auth_mode,
            grok_auth_file: map
                .get("GROK_SEARCH_AUTH_FILE")
                .cloned()
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            grok_model: get(&map, "GROK_SEARCH_MODEL", "grok-4-1-fast-reasoning"),
            web_search_enabled: bool_value(&map, "GROK_SEARCH_WEB_SEARCH", true),
            x_search_enabled: bool_value(&map, "GROK_SEARCH_X_SEARCH", false),
            tavily_api_url: normalize_plain_base(&get(
                &map,
                "TAVILY_API_URL",
                "https://api.tavily.com",
            )),
            // Blank means absent, as it already does for the auth file and the
            // OpenAI-compatible settings below. Kept as `Some("")` these build
            // a provider that exists only to 401 on every call, which makes
            // `doctor` and every availability check report a source that is
            // configured when nothing can come of it. Trimmed, because a
            // whitespace-only key is no more usable than an empty one.
            tavily_api_key: map
                .get("TAVILY_API_KEY")
                .cloned()
                .filter(|value| !value.trim().is_empty()),
            tavily_enabled: bool_value(&map, "TAVILY_ENABLED", true),
            firecrawl_api_url: normalize_v1_base(&get(
                &map,
                "FIRECRAWL_API_URL",
                "https://api.firecrawl.dev",
            )),
            firecrawl_api_key: map
                .get("FIRECRAWL_API_KEY")
                .cloned()
                .filter(|value| !value.trim().is_empty()),
            firecrawl_enabled: bool_value(&map, "FIRECRAWL_ENABLED", true),
            tinyfish_search_api_url: normalize_plain_base(&get(
                &map,
                "TINYFISH_SEARCH_API_URL",
                "https://api.search.tinyfish.ai",
            )),
            tinyfish_fetch_api_url: normalize_plain_base(&get(
                &map,
                "TINYFISH_FETCH_API_URL",
                "https://api.fetch.tinyfish.ai",
            )),
            tinyfish_api_key: map
                .get("TINYFISH_API_KEY")
                .cloned()
                .filter(|value| !value.trim().is_empty()),
            tinyfish_enabled: bool_value(&map, "TINYFISH_ENABLED", true),
            exa_api_url: normalize_plain_base(&get(&map, "EXA_API_URL", "https://api.exa.ai")),
            exa_api_key: map
                .get("EXA_API_KEY")
                .cloned()
                .filter(|value| !value.trim().is_empty()),
            exa_enabled: bool_value(&map, "EXA_ENABLED", true),
            source_providers: csv_list(&map, "GROK_SEARCH_SOURCE_PROVIDERS"),
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
            transport: decide_transport(&map, grok_auth_mode),
            github_token: map.get("GITHUB_TOKEN").cloned().filter(|v| !v.is_empty()),
            source_max_answers: usize_value(&map, "GROK_SEARCH_SOURCE_MAX_ANSWERS", 5),
            source_max_comments: usize_value(&map, "GROK_SEARCH_SOURCE_MAX_COMMENTS", 30),
            enrich_concurrency: usize_value(&map, "GROK_SEARCH_ENRICH_CONCURRENCY", 3).clamp(1, 5),
            enrich_max_chars: usize_value(&map, "GROK_SEARCH_ENRICH_MAX_CHARS", 15000),
            max_inline_sources: usize_value(&map, "GROK_SEARCH_MAX_INLINE_SOURCES", 5),
            response_max_chars: usize_value(&map, "GROK_SEARCH_RESPONSE_MAX_CHARS", 45_000),
            // This is the environment-only constructor; no file was consulted.
            // `load_from` overwrites both after merging one in.
            config_file_path: None,
            config_file_state: ConfigFileState::Absent,
        }
    }

    /// Two-state presence signal for GITHUB_TOKEN. Reports only whether a
    /// token is configured — never the value or any fragment.
    pub fn github_token_status(&self) -> &'static str {
        if self.github_token.is_some() {
            "set"
        } else {
            "unset"
        }
    }

    pub fn redacted_diagnostics(&self) -> String {
        format!(
            "grok_api_url={} grok_api_key={} grok_auth_mode={:?} grok_auth_file={} grok_model={} web_search_enabled={} x_search_enabled={} tavily_api_key={} firecrawl_api_key={} tinyfish_api_key={} exa_api_key={} default_extra_sources={} fallback_sources={} timeout_seconds={} github_token={}",
            self.grok_api_url,
            redact(self.grok_api_key.as_deref()),
            self.grok_auth_mode,
            self.grok_auth_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "default".to_string()),
            self.grok_model,
            self.web_search_enabled,
            self.x_search_enabled,
            redact(self.tavily_api_key.as_deref()),
            redact(self.firecrawl_api_key.as_deref()),
            redact(self.tinyfish_api_key.as_deref()),
            redact(self.exa_api_key.as_deref()),
            self.default_extra_sources,
            self.fallback_sources,
            self.timeout.as_secs(),
            self.github_token_status()
        )
    }
}

fn resolve_config_path(env: &HashMap<String, String>) -> Option<PathBuf> {
    if let Some(explicit) = env.get("GROK_SEARCH_CONFIG").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(explicit));
    }
    let home = resolve_home_dir(env)?;
    Some(
        home.join(".config")
            .join("grok-search-rs")
            .join("config.toml"),
    )
}

/// Cross-platform home directory resolution. Reads `$HOME` first (Unix and
/// Git Bash / MSYS on Windows both set it), then falls back to
/// `%USERPROFILE%` for native Windows shells (PowerShell, cmd) where `HOME`
/// is not part of the default environment. Env-driven so tests can inject
/// either layout without touching real process env.
fn resolve_home_dir(env: &HashMap<String, String>) -> Option<PathBuf> {
    if let Some(home) = env.get("HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home));
    }
    if let Some(profile) = env.get("USERPROFILE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(profile));
    }
    None
}

/// Resolved config file path using process env. Precedence:
/// 1. `$GROK_SEARCH_CONFIG` (any platform, explicit override)
/// 2. `$HOME/.config/grok-search-rs/config.toml` (Unix / Git Bash)
/// 3. `%USERPROFILE%\.config\grok-search-rs\config.toml` (native Windows)
///
/// Returns `None` only when none of the above are set.
pub fn config_path() -> Option<PathBuf> {
    let env: HashMap<String, String> = std::env::vars().collect();
    resolve_config_path(&env)
}

pub fn auth_path() -> Option<PathBuf> {
    auth_path_for(std::env::vars())
}

pub fn auth_path_for<I, K, V>(env_vars: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let env: HashMap<String, String> = env_vars
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    if let Some(explicit) = env.get("GROK_SEARCH_AUTH_FILE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(explicit));
    }
    resolve_config_path(&env).map(|path| path.with_file_name("auth.json"))
}

/// Test-friendly variant of [`config_path`] that takes an explicit env map.
/// Lets integration tests assert path resolution across platforms without
/// mutating process-global env state.
pub fn config_path_for<I, K, V>(env_vars: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let env: HashMap<String, String> = env_vars
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
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
# Default path:
#   Unix / macOS / Git Bash:   $HOME/.config/grok-search-rs/config.toml
#   Windows (PowerShell/cmd):  %USERPROFILE%\.config\grok-search-rs\config.toml
# Override anywhere with $GROK_SEARCH_CONFIG=/abs/path/to/config.toml
#
# Precedence: process env > this file > built-in defaults.
# All keys below are commented out; uncomment and fill what you need.
# Unknown keys are rejected — typos surface as errors, not silent drops.

# ── Required ──────────────────────────────────────────────────
# grok_api_key   = "xai-..."          # xAI / Grok key   https://x.ai/api
# grok_auth_mode = "api_key"          # api_key | oauth
# grok_auth_file = "C:\\Users\\you\\.config\\grok-search-rs\\auth.json"
# tavily_api_key = "tvly-..."         # Tavily key       https://tavily.com
#                                     # comma-separated list rotates keys round-robin
#                                     # e.g. "tvly-a,tvly-b" (failover on 401/429/432/433)

# ── Common knobs ──────────────────────────────────────────────
# grok_model         = "grok-4-1-fast-reasoning"
# x_search_enabled   = false          # Grok X/Twitter search tool
# firecrawl_api_key  = "fc-..."       # Optional fetch fallback   https://firecrawl.dev
# tinyfish_api_key   = "tf-..."       # Optional free search/fetch  https://tinyfish.ai
# exa_api_key        = "exa-..."      # Optional semantic search    https://exa.ai
# source_providers   = ["tavily", "exa", "tinyfish", "firecrawl"]
#                                     # explicit chain order; omit for the
#                                     # built-in order over configured providers

# ── Endpoints (only set when using a self-hosted gateway) ─────
# grok_api_url            = "https://api.x.ai"
# tavily_api_url          = "https://api.tavily.com"
# firecrawl_api_url       = "https://api.firecrawl.dev"
# tinyfish_search_api_url = "https://api.search.tinyfish.ai"
# tinyfish_fetch_api_url  = "https://api.fetch.tinyfish.ai"
# exa_api_url             = "https://api.exa.ai"

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
# tinyfish_enabled   = true
# exa_enabled        = true

# ── Behavior tuning ───────────────────────────────────────────
# default_extra_sources = 3
# fallback_sources      = 5
# fetch_max_chars       = 200000      # per-request char cap on web_fetch
# cache_size            = 256
# timeout_seconds       = 60
# source_max_answers    = 5          # max answers rendered per StackExchange question
# source_max_comments   = 30         # max comments per accepted answer
# github_token          = "ghp_..."  # GitHub token (optional; anon = 60 req/hr)
# enrich_concurrency    = 3          # concurrent resolve_content calls per web_search (1..5)
# enrich_max_chars      = 15000      # per-source inline content char cap
# max_inline_sources    = 5          # max sources carrying inline content per response
# response_max_chars    = 45000      # whole-response char budget (answer + inline content); kept below the MCP client token ceiling (default ~25k tokens) after JSON serialization
"#;

/// Read and parse the config file, reporting what happened alongside whatever
/// values survived. A missing file is `Absent`, not a failure — running purely
/// on environment variables is normal. Anything present but unusable is
/// `Rejected` with the reason attached, so the caller can surface it somewhere
/// the operator will actually look.
fn read_config_file(path: &Path) -> (HashMap<String, String>, ConfigFileState) {
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (HashMap::new(), ConfigFileState::Absent)
        }
        Err(err) => return (HashMap::new(), ConfigFileState::Rejected(err.to_string())),
    };
    match toml::from_str::<ConfigFile>(&body) {
        Ok(file) => (file.into_env_map(), ConfigFileState::Loaded),
        Err(err) => {
            // Kept for anyone running the binary directly; the recorded state
            // is what reaches an MCP client.
            eprintln!(
                "grok-search-rs: ignoring malformed config {}: {}",
                path.display(),
                err
            );
            (HashMap::new(), ConfigFileState::Rejected(err.to_string()))
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
    // Strip any known full-endpoint suffix so callers can pass either a base
    // URL or a full endpoint and converge on the same `/v1` form.
    for suffix in ["/chat/completions", "/responses"] {
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

/// Comma-separated list → trimmed, lowercased entries, de-duplicated with
/// first occurrence winning. Absent/blank values yield an empty list.
fn csv_list(map: &HashMap<String, String>, key: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for part in map
        .get(key)
        .map(String::as_str)
        .unwrap_or_default()
        .split(',')
    {
        let name = part.trim().to_ascii_lowercase();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn bool_literal(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

fn auth_mode_value(map: &HashMap<String, String>) -> AuthMode {
    match map
        .get("GROK_SEARCH_AUTH_MODE")
        .map(|value| (value.trim(), value.trim().to_ascii_lowercase()))
    {
        Some((_, value)) if value == "api_key" || value.is_empty() => AuthMode::ApiKey,
        Some((_, value)) if value == "oauth" => AuthMode::OAuth,
        Some((raw, _)) => {
            eprintln!(
                "unknown GROK_SEARCH_AUTH_MODE=\"{}\"; falling back to api_key. Valid values: api_key, oauth.",
                raw
            );
            AuthMode::ApiKey
        }
        _ => AuthMode::ApiKey,
    }
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

fn decide_transport(map: &HashMap<String, String>, auth_mode: AuthMode) -> Transport {
    if auth_mode == AuthMode::OAuth {
        return Transport::Responses;
    }
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

/// Two-state presence marker for a secret. Never emits any fragment of the
/// value — mirrors [`Config::github_token_status`] so `doctor`/diagnostics on a
/// public endpoint cannot leak even a prefix/suffix of a caller's key.
fn redact(value: Option<&str>) -> String {
    match value {
        Some(v) if !v.trim().is_empty() => "set".to_string(),
        _ => "unset".to_string(),
    }
}

#[cfg(test)]
mod source_config_tests {
    use super::*;

    #[test]
    fn source_caps_defaults_hold() {
        let cfg = Config::from_env_map(Vec::<(String, String)>::new());
        assert_eq!(cfg.source_max_answers, 5);
        assert_eq!(cfg.source_max_comments, 30);
    }

    // A blank key must read as absent, not as a configured provider: kept as
    // `Some("")` it builds a source provider that can only 401, and every
    // availability check downstream then reports a source that cannot work.
    #[test]
    fn blank_source_keys_read_as_absent() {
        let cfg = Config::from_env_map([("TAVILY_API_KEY", ""), ("FIRECRAWL_API_KEY", "   ")]);
        assert_eq!(cfg.tavily_api_key, None);
        assert_eq!(cfg.firecrawl_api_key, None);

        let configured = Config::from_env_map([
            ("TAVILY_API_KEY", "tvly-real"),
            ("FIRECRAWL_API_KEY", "fc-1"),
        ]);
        assert_eq!(configured.tavily_api_key.as_deref(), Some("tvly-real"));
        assert_eq!(configured.firecrawl_api_key.as_deref(), Some("fc-1"));
    }

    #[test]
    fn source_max_answers_reads_env() {
        let cfg = Config::from_env_map([("GROK_SEARCH_SOURCE_MAX_ANSWERS", "3")]);
        assert_eq!(cfg.source_max_answers, 3);
    }

    #[test]
    fn source_max_comments_reads_env() {
        let cfg = Config::from_env_map([("GROK_SEARCH_SOURCE_MAX_COMMENTS", "10")]);
        assert_eq!(cfg.source_max_comments, 10);
    }

    #[test]
    fn github_token_present_and_filtered() {
        let cfg = Config::from_env_map([("GITHUB_TOKEN", "ghp_test")]);
        assert_eq!(cfg.github_token.as_deref(), Some("ghp_test"));

        let empty = Config::from_env_map([("GITHUB_TOKEN", "")]);
        assert_eq!(empty.github_token, None);

        let unset = Config::from_env_map(Vec::<(String, String)>::new());
        assert_eq!(unset.github_token, None);

        // redacted_diagnostics() reports a two-state set|unset signal and
        // NEVER the token value (no redact() masking either).
        let diag_set = cfg.redacted_diagnostics();
        assert!(
            diag_set.contains("github_token=set"),
            "expected github_token=set in: {diag_set}"
        );
        assert!(
            !diag_set.contains("ghp_test"),
            "token value leaked into diagnostics: {diag_set}"
        );

        let diag_unset = unset.redacted_diagnostics();
        assert!(
            diag_unset.contains("github_token=unset"),
            "expected github_token=unset in: {diag_unset}"
        );
    }

    #[test]
    fn debug_does_not_leak_secret_values() {
        let cfg = Config::from_env_map([
            ("GITHUB_TOKEN", "ghp_test"),
            ("GROK_SEARCH_API_KEY", "xai-secret"),
            ("TAVILY_API_KEY", "tvly-secret"),
            ("FIRECRAWL_API_KEY", "fc-secret"),
            ("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-secret"),
        ]);
        let dbg = format!("{cfg:?}");
        for leaked in [
            "ghp_test",
            "xai-secret",
            "tvly-secret",
            "fc-secret",
            "sk-secret",
        ] {
            assert!(
                !dbg.contains(leaked),
                "secret value {leaked} leaked into Debug output: {dbg}"
            );
        }
        // Non-secret fields stay readable.
        assert!(
            dbg.contains("grok_model"),
            "expected readable field in: {dbg}"
        );
        assert!(
            dbg.contains("github_token: \"set\""),
            "expected masked set marker in: {dbg}"
        );
    }

    #[test]
    fn enrich_config_defaults_hold() {
        let cfg = Config::from_env_map(Vec::<(String, String)>::new());
        assert_eq!(cfg.enrich_concurrency, 3);
        assert_eq!(cfg.enrich_max_chars, 15000);
    }

    #[test]
    fn enrich_concurrency_reads_env_and_clamps() {
        let cfg = Config::from_env_map([("GROK_SEARCH_ENRICH_CONCURRENCY", "7")]);
        assert_eq!(cfg.enrich_concurrency, 5); // clamped to 1..=5
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
        assert_eq!(
            cfg.openai_compatible_model.as_deref(),
            Some("grok-4.3-fast")
        );
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
