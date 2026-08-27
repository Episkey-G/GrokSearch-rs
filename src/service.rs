use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::cache::SourceCache;
use crate::config::{AuthMode, Config};
use crate::credentials::{OAuthCredential, StaticApiKeyCredential};
use crate::error::{GrokSearchError, Result};
use crate::model::search::{
    ContentBlock, SearchFilters, SearchMessage, SearchRequest, SearchResponse, SearchTool,
};
use crate::model::source::{is_junk_title, merge_sources, FetchedPage, Source};
use crate::model::tool::{GetSourcesOutput, WebFetchOutput, WebSearchInput, WebSearchOutput};
use crate::providers::exa::ExaProvider;
use crate::providers::firecrawl::FirecrawlProvider;
use crate::providers::grok::GrokResponsesProvider;
use crate::providers::tavily::TavilyProvider;
use crate::providers::tinyfish::TinyfishProvider;

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn search(&self, request: &SearchRequest) -> Result<SearchResponse>;
}

#[async_trait]
pub trait SourceProvider: Send + Sync {
    async fn search_sources(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>>;
    async fn fetch(&self, url: &str) -> Result<FetchedPage>;
    async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>>;
}

#[async_trait]
impl AiProvider for GrokResponsesProvider {
    async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
        GrokResponsesProvider::search(self, request).await
    }
}

#[async_trait]
impl AiProvider for crate::providers::openai_compatible::OpenAICompatProvider {
    async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
        crate::providers::openai_compatible::OpenAICompatProvider::search(self, request).await
    }
}

#[async_trait]
impl SourceProvider for TavilyProvider {
    async fn search_sources(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        self.search(query, max_results, filters).await
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        self.extract(url).await
    }

    async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        self.map(url, max_results).await
    }
}

#[async_trait]
impl SourceProvider for FirecrawlProvider {
    async fn search_sources(
        &self,
        query: &str,
        max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        // Firecrawl search has no structured recency/domain filter; ignore filters.
        FirecrawlProvider::search(self, query, max_results).await
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        FirecrawlProvider::scrape(self, url).await
    }

    async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        FirecrawlProvider::search(self, url, max_results).await
    }
}

#[async_trait]
impl SourceProvider for TinyfishProvider {
    async fn search_sources(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        self.search(query, max_results, filters).await
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        TinyfishProvider::fetch(self, url).await
    }

    async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        // No native site-map endpoint; a search on the URL is the closest
        // discovery primitive (mirrors the Firecrawl impl above).
        self.search(url, max_results, &SearchFilters::default())
            .await
    }
}

#[async_trait]
impl SourceProvider for ExaProvider {
    async fn search_sources(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        self.search(query, max_results, filters).await
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        ExaProvider::fetch(self, url).await
    }

    async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        // No native site-map endpoint; a search on the URL is the closest
        // discovery primitive (mirrors the Firecrawl impl above).
        self.search(url, max_results, &SearchFilters::default())
            .await
    }
}

/// Static description of one known source provider: how it is configured, how
/// its absence is reported, and which request shapes it can honor. The
/// service's source chain (`SourceSlot` list) is built from these at
/// construction time.
pub(crate) struct ProviderSpec {
    pub(crate) name: &'static str,
    display: &'static str,
    enable_var: &'static str,
    key_var: &'static str,
    header: &'static str,
    /// Whether the provider honors `SearchFilters` (recency + domain
    /// include/exclude). Providers that cannot are *skipped* for filtered
    /// requests instead of silently violating the filter contract.
    supports_filters: bool,
    /// Whether the provider exposes a real site-map endpoint (`web_map`).
    supports_map: bool,
    /// Core providers appear in zero-source diagnostics even when
    /// unconfigured; optional ones are mentioned only when the operator names
    /// them in `GROK_SEARCH_SOURCE_PROVIDERS`. Keeps "I never set up Exa"
    /// from reading as a broken credential in every failure message.
    core: bool,
}

const TAVILY_SPEC: ProviderSpec = ProviderSpec {
    name: "tavily",
    display: "Tavily",
    enable_var: "TAVILY_ENABLED",
    key_var: "TAVILY_API_KEY",
    header: "x-tavily-api-key",
    supports_filters: true,
    supports_map: true,
    core: true,
};

const EXA_SPEC: ProviderSpec = ProviderSpec {
    name: "exa",
    display: "Exa",
    enable_var: "EXA_ENABLED",
    key_var: "EXA_API_KEY",
    header: "x-exa-api-key",
    supports_filters: true,
    supports_map: false,
    core: false,
};

const TINYFISH_SPEC: ProviderSpec = ProviderSpec {
    name: "tinyfish",
    display: "TinyFish",
    enable_var: "TINYFISH_ENABLED",
    key_var: "TINYFISH_API_KEY",
    header: "x-tinyfish-api-key",
    supports_filters: true,
    supports_map: false,
    core: false,
};

const FIRECRAWL_SPEC: ProviderSpec = ProviderSpec {
    name: "firecrawl",
    display: "Firecrawl",
    enable_var: "FIRECRAWL_ENABLED",
    key_var: "FIRECRAWL_API_KEY",
    header: "x-firecrawl-api-key",
    supports_filters: false,
    supports_map: false,
    core: true,
};

/// Canonical chain order. Tavily's RAG-tuned results keep the primary slot;
/// Exa (semantic search, native filter support) outranks the keyword engines
/// among the newcomers; TinyFish is the free keyword/fetch tier; Firecrawl
/// keeps its historical last-resort slot because its search cannot honor
/// filters. `GROK_SEARCH_SOURCE_PROVIDERS` overrides this order entirely.
const CANONICAL_SOURCE_ORDER: [&ProviderSpec; 4] =
    [&TAVILY_SPEC, &EXA_SPEC, &TINYFISH_SPEC, &FIRECRAWL_SPEC];

/// One instantiated provider in the source chain.
#[derive(Clone)]
struct SourceEntry {
    spec: &'static ProviderSpec,
    provider: Arc<dyn SourceProvider>,
}

/// One position in the configured source chain: an instantiated provider, or
/// a known-but-absent provider remembered so zero-source failures can name
/// what was missing and how to supply it (`enabled` decides whether the note
/// reads as "switched off" or "no key").
enum SourceSlot {
    Active(SourceEntry),
    Missing {
        spec: &'static ProviderSpec,
        enabled: bool,
    },
}

fn instantiate_source(
    spec: &'static ProviderSpec,
    config: &Config,
    http: &reqwest::Client,
) -> Option<Arc<dyn SourceProvider>> {
    match spec.name {
        "tavily" => config
            .tavily_enabled
            .then(|| config.tavily_api_key.clone())
            .flatten()
            .map(|key| {
                Arc::new(TavilyProvider::with_client(
                    http.clone(),
                    config.tavily_api_url.clone(),
                    key,
                )) as Arc<dyn SourceProvider>
            }),
        "exa" => config
            .exa_enabled
            .then(|| config.exa_api_key.clone())
            .flatten()
            .map(|key| {
                Arc::new(ExaProvider::with_client(
                    http.clone(),
                    config.exa_api_url.clone(),
                    key,
                )) as Arc<dyn SourceProvider>
            }),
        "tinyfish" => config
            .tinyfish_enabled
            .then(|| config.tinyfish_api_key.clone())
            .flatten()
            .map(|key| {
                Arc::new(TinyfishProvider::with_client(
                    http.clone(),
                    config.tinyfish_search_api_url.clone(),
                    config.tinyfish_fetch_api_url.clone(),
                    key,
                )) as Arc<dyn SourceProvider>
            }),
        "firecrawl" => config
            .firecrawl_enabled
            .then(|| config.firecrawl_api_key.clone())
            .flatten()
            .map(|key| {
                Arc::new(FirecrawlProvider::with_client(
                    http.clone(),
                    config.firecrawl_api_url.clone(),
                    key,
                )) as Arc<dyn SourceProvider>
            }),
        _ => None,
    }
}

fn provider_enabled(spec: &ProviderSpec, config: &Config) -> bool {
    match spec.name {
        "tavily" => config.tavily_enabled,
        "exa" => config.exa_enabled,
        "tinyfish" => config.tinyfish_enabled,
        "firecrawl" => config.firecrawl_enabled,
        _ => false,
    }
}

/// Resolve the effective source chain. Default: the canonical order over
/// whatever is configured, with core providers (Tavily/Firecrawl) holding
/// `Missing` slots when absent — preserving their long-standing place in
/// zero-source diagnostics. With an explicit `GROK_SEARCH_SOURCE_PROVIDERS`
/// list, exactly the named providers are slotted, and every named-but-absent
/// one gets a `Missing` slot: the operator asked for it, so its absence is
/// worth reporting.
/// Check every name in `GROK_SEARCH_SOURCE_PROVIDERS` against the known
/// provider registry. Exposed separately from [`build_source_slots`] so the
/// HTTP transport can fail at startup (before binding the listener) instead
/// of failing every request later — the operator's chain is operator-fixed
/// config, never a per-request header, so a startup check covers it fully.
pub fn validate_source_providers(config: &Config) -> Result<()> {
    for name in &config.source_providers {
        if !CANONICAL_SOURCE_ORDER.iter().any(|spec| spec.name == name) {
            return Err(GrokSearchError::InvalidParams(format!(
                "unknown source provider \"{name}\" in GROK_SEARCH_SOURCE_PROVIDERS (valid: tavily, exa, tinyfish, firecrawl)"
            )));
        }
    }
    Ok(())
}

fn build_source_slots(config: &Config, http: &reqwest::Client) -> Result<Vec<SourceSlot>> {
    validate_source_providers(config)?;
    let explicit = !config.source_providers.is_empty();
    let specs: Vec<&'static ProviderSpec> = if explicit {
        config
            .source_providers
            .iter()
            .filter_map(|name| {
                CANONICAL_SOURCE_ORDER
                    .iter()
                    .copied()
                    .find(|spec| spec.name == name)
            })
            .collect()
    } else {
        CANONICAL_SOURCE_ORDER.to_vec()
    };

    let mut slots = Vec::new();
    for spec in specs {
        match instantiate_source(spec, config, http) {
            Some(provider) => slots.push(SourceSlot::Active(SourceEntry { spec, provider })),
            None if spec.core || explicit => slots.push(SourceSlot::Missing {
                spec,
                enabled: provider_enabled(spec, config),
            }),
            None => {}
        }
    }
    Ok(slots)
}

#[derive(Clone)]
pub struct SearchService {
    config: Config,
    ai: Arc<dyn AiProvider>,
    /// Model name written into every `SearchRequest` produced by the service.
    /// Resolved once from `config` at construction so each transport gets the
    /// model it actually understands: `grok_model` for Responses, and
    /// `openai_compatible_model` (falling back to `grok_model`) for the
    /// chat-completions transport. Per-call overrides via `WebSearchInput.model`
    /// still win.
    default_model: String,
    /// Ordered source-provider chain (first usable answer wins), with
    /// `Missing` placeholders so diagnostics can name absent providers.
    /// Behind `Arc` so `SearchService: Clone` stays cheap.
    source_slots: Arc<Vec<SourceSlot>>,
    cache: Arc<Mutex<SourceCache>>,
    /// Shared reqwest client for the sources pipeline (same instance handed to
    /// providers). Stored here because resolve_content needs direct GET access.
    http_client: reqwest::Client,
    /// Specialist extractor router. Empty in Phase 1. Behind `Arc` so
    /// `SearchService: Clone` still holds (the router is not `Clone`).
    source_router: Arc<crate::sources::SourceRouter>,
}

/// The credential-derived half of a [`SearchService`]: everything that must be
/// rebuilt when the caller's keys change. [`build_providers`] constructs these
/// from a [`Config`]; [`SearchService::new`] uses it with a process-wide config
/// (the stdio path), and [`SearchService::with_config`] uses it per request
/// while sharing the long-lived HTTP client and source cache.
struct ProviderSet {
    ai: Arc<dyn AiProvider>,
    default_model: String,
    source_slots: Vec<SourceSlot>,
    source_router: Arc<crate::sources::SourceRouter>,
}

/// Build the credential-bearing providers for a given `config`, reusing the
/// caller-supplied shared `http` client. Extracted verbatim from the original
/// `SearchService::new` body so both the process-wide (stdio) and per-request
/// (HTTP) construction paths share one implementation.
fn build_providers(config: &Config, http: &reqwest::Client) -> Result<ProviderSet> {
    build_providers_with_grok(config, http, http)
}

/// Like [`build_providers`], but the Grok **Responses** provider uses
/// `grok_http` while every other provider (Tavily / Firecrawl / source
/// fetching) keeps `http`. The HTTP transport passes a DNS-pinned,
/// no-redirect client as `grok_http` for a caller-supplied gateway
/// (`X-Grok-Base-Url`) — that restriction must apply to the gateway request
/// only, not to unrelated fetch/search traffic.
fn build_providers_with_grok(
    config: &Config,
    http: &reqwest::Client,
    grok_http: &reqwest::Client,
) -> Result<ProviderSet> {
    use crate::config::Transport;

    let ai: Arc<dyn AiProvider> = match config.transport {
        Transport::Responses => {
            let credential: Arc<dyn crate::credentials::CredentialProvider> =
                match config.grok_auth_mode {
                    AuthMode::ApiKey => Arc::new(StaticApiKeyCredential::new(
                        config
                            .grok_api_key
                            .clone()
                            .ok_or(GrokSearchError::MissingConfig("GROK_SEARCH_API_KEY"))?,
                    )),
                    AuthMode::OAuth => {
                        let auth_path = config
                            .grok_auth_file
                            .clone()
                            .or_else(crate::config::auth_path)
                            .ok_or_else(|| {
                                GrokSearchError::OAuth(
                                    "oauth_auth_path_unavailable: set GROK_SEARCH_AUTH_FILE"
                                        .to_string(),
                                )
                            })?;
                        Arc::new(OAuthCredential::new(http.clone(), auth_path))
                    }
                };
            Arc::new(GrokResponsesProvider::with_credential_client(
                grok_http.clone(),
                config.grok_api_url.clone(),
                credential,
                config.web_search_enabled,
                config.x_search_enabled,
            ))
        }
        Transport::ChatCompletions => {
            let url = config
                .openai_compatible_api_url
                .clone()
                .ok_or(GrokSearchError::MissingConfig("OPENAI_COMPATIBLE_API_URL"))?;
            let key = config
                .openai_compatible_api_key
                .clone()
                .ok_or(GrokSearchError::MissingConfig("OPENAI_COMPATIBLE_API_KEY"))?;
            let model = config
                .openai_compatible_model
                .clone()
                .unwrap_or_else(|| config.grok_model.clone());
            if config.x_search_enabled {
                eprintln!(
                    "grok-search-rs: x_search_enabled is ignored when using OPENAI_COMPATIBLE_* transport"
                );
            }
            Arc::new(
                crate::providers::openai_compatible::OpenAICompatProvider::with_client(
                    http.clone(),
                    url,
                    key,
                    model,
                    config.web_search_enabled,
                ),
            )
        }
    };

    let source_slots = build_source_slots(config, http)?;

    let source_router = Arc::new(crate::sources::SourceRouter::from_config(config));

    Ok(ProviderSet {
        ai,
        default_model: resolve_default_model(config),
        source_slots,
        source_router,
    })
}

/// Non-reversible per-tenant namespace tag derived from the caller's primary
/// key. Cache entries are stored under `tag:session_id` so one tenant can never
/// read another tenant's cached `get_sources` pages on the shared HTTP process.
/// For stdio (a single process key) the tag is constant, so behavior is
/// unchanged. The gateway URL is part of the hash material: with arbitrary
/// public gateways two tenants on different gateways may present the same
/// opaque key string, and they must not share a cache namespace. Uses a
/// SHA-256 prefix — never any fragment of the raw key.
fn tenant_tag(config: &Config) -> String {
    let key = config
        .grok_api_key
        .as_deref()
        .or(config.openai_compatible_api_key.as_deref())
        .unwrap_or("");
    if key.is_empty() {
        return "anon".to_string();
    }
    let material = format!("{}\n{}", config.grok_api_url, key);
    let digest = ring::digest::digest(&ring::digest::SHA256, material.as_bytes());
    digest.as_ref()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl SearchService {
    pub fn new(config: Config) -> Result<Self> {
        let http = crate::providers::http::build_client(config.timeout);
        let providers = build_providers(&config, &http)?;
        let cache = Arc::new(Mutex::new(SourceCache::new(config.cache_size)));
        Ok(Self::from_parts(config, http, cache, providers))
    }

    /// Build a request-scoped service that reuses this service's shared HTTP
    /// client and source cache, but derives every credential-bearing provider
    /// from `config`. The HTTP transport calls this per request so each caller
    /// searches with their own keys while the process keeps a single source
    /// cache — so `get_sources` continuation still works across requests.
    ///
    /// OAuth is rejected here: it resolves a single on-disk identity and is
    /// incompatible with per-request, multi-tenant credentials. HTTP callers
    /// must pass an API key. The local stdio path keeps OAuth via [`new`].
    ///
    /// [`new`]: SearchService::new
    pub fn with_config(&self, config: Config) -> Result<Self> {
        Self::for_request(self.http_client.clone(), self.cache.clone(), config)
    }

    /// Build a request-scoped service from shared state — a reused HTTP client
    /// and the process-wide source cache — plus a per-request `config`. This is
    /// the entrypoint the HTTP transport uses: the server process holds no
    /// credentials of its own, so it keeps only the shared client + cache and
    /// constructs a fully-credentialed service per request from the caller's
    /// header keys. OAuth is rejected here (single on-disk identity is
    /// incompatible with per-request multi-tenancy); a missing required key
    /// fails at construction (fail-closed) rather than reusing any server key.
    pub fn for_request(
        http_client: reqwest::Client,
        cache: Arc<Mutex<SourceCache>>,
        config: Config,
    ) -> Result<Self> {
        Self::for_request_with_grok_client(http_client.clone(), http_client, cache, config)
    }

    /// Like [`for_request`], but the Grok provider uses `grok_client` while all
    /// other providers keep `http_client`. The HTTP transport passes a
    /// DNS-pinned, no-redirect client here for a caller-supplied gateway, so
    /// the pin/no-redirect restriction stays scoped to the gateway request and
    /// never degrades unrelated fetch/search redirect handling.
    ///
    /// [`for_request`]: SearchService::for_request
    pub fn for_request_with_grok_client(
        http_client: reqwest::Client,
        grok_client: reqwest::Client,
        cache: Arc<Mutex<SourceCache>>,
        config: Config,
    ) -> Result<Self> {
        if config.grok_auth_mode == AuthMode::OAuth {
            return Err(GrokSearchError::OAuth(
                "oauth is not supported on the HTTP transport; pass a per-request API key"
                    .to_string(),
            ));
        }
        let providers = build_providers_with_grok(&config, &http_client, &grok_client)?;
        Ok(Self::from_parts(config, http_client, cache, providers))
    }

    /// Assemble a `SearchService` from an already-built provider set plus the
    /// shared `http` client and `cache`. Single assembly point for both `new`
    /// (fresh client + cache) and `with_config` (shared client + cache).
    fn from_parts(
        config: Config,
        http: reqwest::Client,
        cache: Arc<Mutex<SourceCache>>,
        providers: ProviderSet,
    ) -> Self {
        Self {
            cache,
            default_model: providers.default_model,
            config,
            ai: providers.ai,
            source_slots: Arc::new(providers.source_slots),
            http_client: http,
            source_router: providers.source_router,
        }
    }

    /// Instantiated providers from the chain, in chain order.
    fn active_sources(&self) -> Vec<SourceEntry> {
        self.source_slots
            .iter()
            .filter_map(|slot| match slot {
                SourceSlot::Active(entry) => Some(entry.clone()),
                SourceSlot::Missing { .. } => None,
            })
            .collect()
    }

    /// Namespace a session id with the caller's tenant tag so cached
    /// `get_sources` pages are isolated per tenant. The plain `session_id`
    /// returned to the caller is unchanged; only the internal cache key is
    /// prefixed.
    fn tenant_cache_key(&self, session_id: &str) -> String {
        format!("{}:{}", tenant_tag(&self.config), session_id)
    }

    pub fn fake_with_sources() -> Self {
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "fake-grok"),
            ("TAVILY_API_KEY", "fake-tavily"),
        ]);
        let firecrawl_enabled = config.firecrawl_enabled;
        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(vec![
                SourceSlot::Active(SourceEntry {
                    spec: &TAVILY_SPEC,
                    provider: Arc::new(FakeSourceProvider),
                }),
                SourceSlot::Missing {
                    spec: &FIRECRAWL_SPEC,
                    enabled: firecrawl_enabled,
                },
            ]),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        }
    }

    /// Legacy two-slot wiring for test factories: `primary` occupies the
    /// Tavily slot and `fallback` the Firecrawl slot, exactly as the
    /// pre-chain service was shaped.
    fn fake_slots(
        primary: Arc<dyn SourceProvider>,
        fallback: Option<Arc<dyn SourceProvider>>,
        firecrawl_enabled: bool,
    ) -> Arc<Vec<SourceSlot>> {
        let firecrawl_slot = match fallback {
            Some(provider) => SourceSlot::Active(SourceEntry {
                spec: &FIRECRAWL_SPEC,
                provider,
            }),
            None => SourceSlot::Missing {
                spec: &FIRECRAWL_SPEC,
                enabled: firecrawl_enabled,
            },
        };
        Arc::new(vec![
            SourceSlot::Active(SourceEntry {
                spec: &TAVILY_SPEC,
                provider: primary,
            }),
            firecrawl_slot,
        ])
    }

    /// Unified test factory: override AI / primary / fallback providers and
    /// inject extra env vars. Use `fake_with_sources()` for the trivial case.
    pub fn fake_custom<I, K, V>(
        ai: Option<Arc<dyn AiProvider>>,
        primary: Arc<dyn SourceProvider>,
        fallback: Option<Arc<dyn SourceProvider>>,
        overrides: I,
    ) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut vars = vec![
            ("GROK_SEARCH_API_KEY".to_string(), "fake-grok".to_string()),
            ("TAVILY_API_KEY".to_string(), "fake-tavily".to_string()),
        ];
        if fallback.is_some() {
            vars.push((
                "FIRECRAWL_API_KEY".to_string(),
                "fake-firecrawl".to_string(),
            ));
        }
        vars.extend(
            overrides
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        let config = Config::from_env_map(vars);

        let source_slots = Self::fake_slots(primary, fallback, config.firecrawl_enabled);
        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            default_model: resolve_default_model(&config),
            config,
            ai: ai.unwrap_or_else(|| Arc::new(FakeAiProvider)),
            source_slots,
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        }
    }

    /// Test factory that injects a populated [`crate::sources::SourceRouter`] so
    /// fallback behavior can be exercised with fake extractors. Mirrors
    /// `fake_custom`'s provider wiring.
    pub fn fake_with_router(
        primary: Arc<dyn SourceProvider>,
        fallback: Option<Arc<dyn SourceProvider>>,
        router: crate::sources::SourceRouter,
    ) -> Self {
        let mut vars = vec![
            ("GROK_SEARCH_API_KEY".to_string(), "fake-grok".to_string()),
            ("TAVILY_API_KEY".to_string(), "fake-tavily".to_string()),
        ];
        if fallback.is_some() {
            vars.push((
                "FIRECRAWL_API_KEY".to_string(),
                "fake-firecrawl".to_string(),
            ));
        }
        let config = Config::from_env_map(vars);
        let source_slots = Self::fake_slots(primary, fallback, config.firecrawl_enabled);
        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots,
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(router),
        }
    }

    pub async fn web_search(&self, input: WebSearchInput) -> Result<WebSearchOutput> {
        // D-02: single global deadline shared by Grok + supplemental fetch + enrichment.
        let deadline = tokio::time::Instant::now() + self.config.timeout;
        // response_format (Anthropic tool-design guidance: concise|detailed)
        // wins over the legacy include_content flag when both are present.
        let format_include_content = match input.response_format.as_deref() {
            None => None,
            Some("concise") => Some(false),
            Some("detailed") => Some(true),
            Some(other) => {
                return Err(GrokSearchError::InvalidParams(format!(
                    "response_format must be \"concise\" or \"detailed\", got \"{other}\""
                )))
            }
        };
        let include_content =
            format_include_content.unwrap_or_else(|| input.include_content.unwrap_or(true));

        let mut uuid_buf = [0u8; uuid::fmt::Simple::LENGTH];
        let session_id = {
            let encoded = Uuid::new_v4().simple().encode_lower(&mut uuid_buf);
            encoded[..12].to_string()
        };
        let effective_extra_sources = input
            .extra_sources
            .unwrap_or(self.config.default_extra_sources);

        let filters = SearchFilters {
            recency_days: input.recency_days,
            include_domains: input.include_domains.clone(),
            exclude_domains: input.exclude_domains.clone(),
        };

        // Speculative fan-out: fetch enough sources to satisfy whichever path
        // (enrichment or fallback) the Grok response routes us into. The
        // speculative call fires concurrently with Grok via tokio::join!, so
        // total latency is roughly max(Grok, Tavily) instead of the sum. The
        // single source call is then sliced to either `effective_extra_sources`
        // (enrichment) or `self.config.fallback_sources` (fallback), preserving
        // the legacy "exactly one source provider call per web_search" contract.
        let speculative_count = effective_extra_sources.max(self.config.fallback_sources);
        let request = self.build_search_request(&input, &[]);

        let grok_future = self.ai.search(&request);
        let speculative_future =
            self.fetch_raw_extra_sources(&input.query, speculative_count, &filters, deadline);
        let (grok_result, raw) = tokio::join!(grok_future, speculative_future);

        let response = match grok_result {
            Ok(response) => response,
            Err(err) => {
                return self
                    .finalize_fallback(
                        deadline,
                        session_id,
                        SearchResponse {
                            content: String::new(),
                            sources: Vec::new(),
                        },
                        raw,
                        grok_error_reason(&err),
                        include_content,
                    )
                    .await;
            }
        };

        if let Some(reason) = grok_unverifiable_reason(&response) {
            return self
                .finalize_fallback(deadline, session_id, response, raw, reason, include_content)
                .await;
        }

        let mut enrichment = raw.sources;
        enrichment.truncate(effective_extra_sources);
        let enrichment = with_provider(enrichment, enrichment_label(raw.origin));
        let merged = merge_sources(response.sources, enrichment);
        // SRCH-04 dual gate (zero-regression): skip enrichment when the caller
        // opted out OR there are no supplemental sources. Gating on
        // include_content alone would leave content populated at extra_sources=0
        // and break the legacy "summary + source list" shape.
        let merged = if include_content && effective_extra_sources > 0 {
            enrich_sources(
                merged,
                deadline,
                &self.http_client,
                &self.source_router,
                crate::sources::SourceCaps {
                    max_answers: self.config.source_max_answers,
                    max_comments: self.config.source_max_comments,
                },
                self.config.enrich_concurrency,
                self.config.enrich_max_chars,
                self.config.max_inline_sources,
                self.active_sources(),
            )
            .await
        } else {
            merged
        };

        let merged_arc = Arc::new(merged);
        let sources_count = merged_arc.len();
        let cache_key = self.tenant_cache_key(&session_id);
        self.cache.lock().await.set(cache_key, merged_arc.clone());

        // The cache keeps the full enriched content; only the returned copy is
        // trimmed to the response budget so drill-down loses nothing.
        let mut out_sources = (*merged_arc).clone();
        let truncated = apply_response_budget(
            response.content.chars().count(),
            &mut out_sources,
            self.config.response_max_chars,
            &session_id,
        );

        Ok(WebSearchOutput {
            session_id,
            content: response.content,
            sources_count,
            sources: out_sources,
            search_provider: "grok_responses".to_string(),
            fallback_used: false,
            fallback_reason: None,
            truncated,
        })
    }

    /// Fetch sources by walking the configured provider chain in order; the
    /// first provider with usable results wins. No path-specific provider
    /// label is applied here — the returned Vec carries each provider's
    /// native label ("tavily"/"exa"/…); the caller re-labels via
    /// `with_provider` once the path (enrichment vs fallback) is known.
    ///
    /// Every path that yields nothing records *why* in [`RawSources::notes`].
    /// Provider errors used to be swallowed outright, which left a request that
    /// ended with zero sources indistinguishable from one where every upstream
    /// was simply out of results — the operator had no way to tell an unset key
    /// from a rate-limited one.
    async fn fetch_raw_extra_sources(
        &self,
        query: &str,
        count: usize,
        filters: &SearchFilters,
        deadline: tokio::time::Instant,
    ) -> RawSources {
        if count == 0 {
            return RawSources::empty(vec![SourceNote::config(
                "source fan-out disabled (extra_sources and fallback_sources are both 0)",
            )]);
        }
        let mut notes = Vec::new();
        for slot in self.source_slots.iter() {
            match slot {
                SourceSlot::Active(entry) => {
                    let name = entry.spec.name;
                    // A provider that ignores `SearchFilters` (Firecrawl) must
                    // never serve a filter-constrained request: swapping
                    // constrained results for unconstrained ones would
                    // silently violate the include/exclude/recency contract.
                    // Filtered requests are filter-capable-providers-or-nothing.
                    if !filters.is_empty() && !entry.spec.supports_filters {
                        notes.push(SourceNote::no_results(format!(
                            "{name}: skipped (request carries domain/recency filters, which {name} cannot honor)"
                        )));
                        continue;
                    }
                    // D-02: the whole chain shares the request's global
                    // deadline. Without this, every chain position gets a
                    // fresh full client timeout and a slow chain multiplies
                    // the configured budget by its length.
                    let attempt = tokio::time::timeout_at(
                        deadline,
                        entry.provider.search_sources(query, count, filters),
                    )
                    .await;
                    let outcome = match attempt {
                        Ok(outcome) => outcome,
                        Err(_elapsed) => {
                            // Later providers would hit the same expired
                            // deadline immediately; one note explains the stop.
                            notes.push(SourceNote::broken(format!(
                                "{name}: timed out (request deadline reached; providers after it were not tried)"
                            )));
                            break;
                        }
                    };
                    match outcome {
                        Ok(sources) => match usable_or_note(name, sources) {
                            Ok(usable) => {
                                if !notes.is_empty() {
                                    eprintln!(
                                        "grok-search-rs: source chain fell through to {name} ({})",
                                        source_diagnosis(&notes)
                                    );
                                }
                                return RawSources::found(usable, Some(name));
                            }
                            Err(note) => notes.push(note),
                        },
                        Err(err) => notes.push(SourceNote::broken(format!("{name}: {err}"))),
                    }
                }
                SourceSlot::Missing { spec, enabled } => {
                    let unavailable = unavailable_note(
                        spec.name,
                        *enabled,
                        spec.enable_var,
                        spec.key_var,
                        spec.header,
                    );
                    // For a filter-blind provider, whether the gate is the
                    // *only* thing in the way decides what to advise. A
                    // configured Firecrawl really would have answered, so
                    // dropping the filters is a genuine remedy; an absent one
                    // changes nothing when the filters go, so the note names
                    // both problems instead of hiding the config one.
                    notes.push(if !filters.is_empty() && !spec.supports_filters {
                        SourceNote::new(
                            unavailable.kind,
                            format!(
                                "{} — also skipped (request carries domain/recency filters, which {} cannot honor)",
                                unavailable.detail, spec.name
                            ),
                        )
                    } else {
                        unavailable
                    });
                }
            }
        }
        RawSources::empty(notes)
    }

    #[allow(clippy::too_many_arguments)]
    async fn finalize_fallback(
        &self,
        deadline: tokio::time::Instant,
        session_id: String,
        response: SearchResponse,
        raw: RawSources,
        reason: &str,
        include_content: bool,
    ) -> Result<WebSearchOutput> {
        let RawSources {
            sources,
            origin,
            notes,
        } = raw;
        let mut fallback = sources;
        fallback.truncate(self.config.fallback_sources);
        let fallback = with_provider(fallback, fallback_label(origin));
        // A metadata-only Grok response — citations but no prose — routes here
        // as `grok_content_empty`, and its citations are real evidence. Merge
        // them rather than dropping them: they keep their `grok_responses`
        // label, exactly as on the success path.
        let fallback = merge_sources(response.sources, fallback);

        // A degraded response with zero sources carries no evidence *and* no
        // answer — the original Grok text is deliberately not echoed on this
        // path. Returning `Ok` here handed the caller a successful-looking
        // empty result, which client models read as "the tool works, this
        // query just missed" and answer by retrying the same dead path
        // forever. Fail loudly instead, naming the upstream reason, what each
        // source provider did, and what would actually change the outcome.
        if fallback.is_empty() {
            // A zero budget truncates the fan-out to nothing whatever the
            // providers returned, so it outranks every other account of the
            // failure: no reformulated query can rescue this path while it
            // holds. Testing the config directly rather than inferring it from
            // an empty source list is what keeps that true when the providers
            // also came back empty.
            return Err(GrokSearchError::Provider(
                if self.config.fallback_sources == 0 {
                    // Providers that were consulted still get to say what they
                    // saw; when the budget alone is at fault there are no notes
                    // and nothing to append.
                    let detail = if notes.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", source_diagnosis(&notes))
                    };
                    format!(
                        "web_search returned no sources: {reason}, and the fallback source budget is 0 (GROK_SEARCH_FALLBACK_SOURCES), so nothing from the source fan-out can survive{detail}. Raise that budget to get evidence on this path; no change of query can help while it is 0."
                    )
                } else {
                    format!(
                        "web_search returned no sources: {reason}, and the source fallback produced nothing ({}). {}",
                        source_diagnosis(&notes),
                        fallback_remediation(&notes)
                    )
                },
            ));
        }

        // D-03: the degraded path enriches eagerly — one-hand evidence is most
        // valuable when there is no verifiable summary, so there is no
        // extra_sources gate here (that gate is the normal web_search path's
        // concern, SRCH-04). The one exception is an explicit include_content=false
        // opt-out, which must be honored everywhere so callers who disabled inline
        // content never pay the extra fetch budget.
        let fallback = if include_content {
            enrich_sources(
                fallback,
                deadline,
                &self.http_client,
                &self.source_router,
                crate::sources::SourceCaps {
                    max_answers: self.config.source_max_answers,
                    max_comments: self.config.source_max_comments,
                },
                self.config.enrich_concurrency,
                self.config.enrich_max_chars,
                self.config.max_inline_sources,
                self.active_sources(),
            )
            .await
        } else {
            fallback
        };

        let fallback_arc = Arc::new(fallback);
        let sources_count = fallback_arc.len();
        let cache_key = self.tenant_cache_key(&session_id);
        self.cache.lock().await.set(cache_key, fallback_arc.clone());

        let content = if response.content.trim().is_empty() {
            format!(
                "Grok Responses search did not return a verifiable answer. Source fallback returned {sources_count} source(s); evaluate them directly rather than treating any text as a verified answer."
            )
        } else {
            format!(
                "Grok Responses returned an answer without verifiable search sources, so source fallback returned {sources_count} source(s). Original Grok answer was not treated as verified; evaluate the listed sources directly."
            )
        };

        let mut out_sources = (*fallback_arc).clone();
        let truncated = apply_response_budget(
            content.chars().count(),
            &mut out_sources,
            self.config.response_max_chars,
            &session_id,
        );

        Ok(WebSearchOutput {
            session_id,
            content,
            sources_count,
            sources: out_sources,
            search_provider: "source_fallback".to_string(),
            fallback_used: true,
            fallback_reason: Some(reason.to_string()),
            truncated,
        })
    }

    /// Return one page of cached sources for a prior `web_search` session.
    /// `offset`/`limit` follow the official MCP fetch server's `start_index`
    /// continuation pattern, applied to sources; an offset past the end is an
    /// empty page, not an error. Each page is additionally subject to the
    /// response budget (`truncated` reports in-page trimming).
    pub async fn get_sources(
        &self,
        session_id: &str,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<GetSourcesOutput> {
        let cached = self
            .cache
            .lock()
            .await
            .get(&self.tenant_cache_key(session_id))
            .ok_or_else(|| GrokSearchError::NotFound(format!("session_id={session_id}")))?;
        let total_sources = cached.len();
        let start = offset.min(total_sources);
        let end = limit
            .map_or(total_sources, |l| start.saturating_add(l))
            .min(total_sources);
        let mut page: Vec<Source> = cached[start..end].to_vec();
        let truncated =
            apply_response_budget(0, &mut page, self.config.response_max_chars, session_id);
        // Budget trimming may shorten the page; continue from what was
        // actually returned, not from the requested slice end.
        let served_end = start + page.len();
        Ok(GetSourcesOutput {
            session_id: session_id.to_string(),
            sources_count: page.len(),
            sources: page,
            total_sources,
            offset,
            next_offset: (served_end < total_sources).then_some(served_end),
            truncated,
        })
    }

    pub async fn web_fetch(&self, url: &str, max_chars: Option<usize>) -> Result<WebFetchOutput> {
        let effective_limit = max_chars.or(self.config.fetch_max_chars);
        // D-02, as on the web_search path: one deadline for the whole call.
        // The specialist attempt and every generic-chain provider draw from
        // the same budget, so a slow specialist followed by a slow chain
        // cannot spend a full GROK_SEARCH_TIMEOUT_SECONDS apiece.
        let deadline = tokio::time::Instant::now() + self.config.timeout;

        let (content, source_type, fallback_reason) = match url::Url::parse(url) {
            Ok(parsed) => {
                let caps = crate::sources::SourceCaps {
                    max_answers: self.config.source_max_answers,
                    max_comments: self.config.source_max_comments,
                };
                let specialist = crate::sources::resolve_content(
                    &self.http_client,
                    &parsed,
                    self.source_router.as_ref(),
                    &caps,
                );
                match tokio::time::timeout_at(deadline, specialist).await {
                    // Specialist succeeded — keep its content and source type.
                    Ok(Ok((content, kind))) => (content, kind, None),
                    // No specialist matched: go generic silently (D-01).
                    Ok(Err(reason)) if reason == crate::sources::NO_SPECIALIST_MATCH => {
                        let generic = self.web_fetch_raw(url, deadline).await?;
                        (generic, crate::sources::SourceType::Generic, None)
                    }
                    // Specialist matched but failed/empty: surface the reason (D-01).
                    Ok(Err(reason)) => {
                        let generic = self.web_fetch_raw(url, deadline).await?;
                        (generic, crate::sources::SourceType::Generic, Some(reason))
                    }
                    // The budget is gone, so the generic chain cannot run
                    // either — report the timeout instead of pretending there
                    // is another path left to try.
                    Err(_elapsed) => {
                        return Err(GrokSearchError::Timeout(format!(
                            "web_fetch timed out extracting {url} (GROK_SEARCH_TIMEOUT_SECONDS)"
                        )))
                    }
                }
            }
            // Malformed URL is not a specialist failure — go generic, no reason.
            Err(_) => {
                let generic = self.web_fetch_raw(url, deadline).await?;
                (generic, crate::sources::SourceType::Generic, None)
            }
        };

        Ok(apply_fetch_limit(
            url,
            content,
            effective_limit,
            source_type,
            fallback_reason,
        ))
    }

    /// Generic fetch through the source chain, bounded by the call's shared
    /// `deadline`. Mirrors how inline enrichment already wraps the same
    /// helper: without this, each chain position would get its own full
    /// client timeout and the chain would multiply the configured budget.
    async fn web_fetch_raw(&self, url: &str, deadline: tokio::time::Instant) -> Result<String> {
        let chain = self.active_sources();
        match tokio::time::timeout_at(deadline, generic_source_fetch(&chain, url)).await {
            Ok(result) => result.map(|page| page.content),
            Err(_elapsed) => Err(GrokSearchError::Timeout(format!(
                "web_fetch timed out fetching {url} through the source chain (GROK_SEARCH_TIMEOUT_SECONDS)"
            ))),
        }
    }

    pub async fn web_map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        // Only providers with a real site-map endpoint qualify (Tavily today);
        // the search-shaped `map` impls on the other providers are not a
        // substitute for actual URL discovery. web_map is a dedicated
        // capability, not part of the supplemental-source chain — an operator
        // whose GROK_SEARCH_SOURCE_PROVIDERS excludes Tavily from the chain
        // keeps map as long as TAVILY_API_KEY is configured, so a map-capable
        // provider absent from the chain is instantiated directly.
        let provider = self
            .source_slots
            .iter()
            .find_map(|slot| match slot {
                SourceSlot::Active(entry) if entry.spec.supports_map => {
                    Some(entry.provider.clone())
                }
                _ => None,
            })
            .or_else(|| instantiate_source(&TAVILY_SPEC, &self.config, &self.http_client))
            .ok_or(GrokSearchError::MissingConfig("TAVILY_API_KEY"))?;
        provider.map(url, max_results).await
    }

    /// Runtime diagnostics with live connectivity probes against each configured backend.
    /// Returns provider availability flags, masked config, and per-provider reachability.
    pub async fn doctor(&self) -> serde_json::Value {
        use crate::config::Transport;
        let grok_probe = self.probe_grok().await;
        let tavily_probe = self.probe_chain_source("tavily").await;
        let exa_probe = self.probe_chain_source("exa").await;
        let tinyfish_probe = self.probe_chain_source("tinyfish").await;
        let firecrawl_probe = self.probe_chain_source("firecrawl").await;
        let source_chain: Vec<&str> = self
            .source_slots
            .iter()
            .filter_map(|slot| match slot {
                SourceSlot::Active(entry) => Some(entry.spec.name),
                SourceSlot::Missing { .. } => None,
            })
            .collect();

        // Surface the AI transport that the service actually dispatches to so
        // doctor() stays truthful when callers point us at an OpenAI-compatible
        // gateway. The legacy "grok" node name is preserved for backward
        // compatibility, but its fields are now sourced from `default_model`
        // and the transport-appropriate API URL — never silently from
        // `grok_model` / `grok_api_url` on the chat-completions path.
        let (provider_label, ai_api_url, ai_x_search_enabled) = match self.config.transport {
            Transport::Responses => (
                "grok_responses",
                self.config.grok_api_url.as_str(),
                self.config.x_search_enabled,
            ),
            Transport::ChatCompletions => (
                "openai_compatible",
                self.config
                    .openai_compatible_api_url
                    .as_deref()
                    .unwrap_or(""),
                // x_search is silently ignored on the chat-completions transport
                // (the gateway has no equivalent); report it as disabled rather
                // than leaking a misleading config flag.
                false,
            ),
        };

        serde_json::json!({
            "provider": provider_label,
            "transport": provider_label,
            "grok": {
                "api_url": ai_api_url,
                "model": self.default_model,
                "auth_mode": match self.config.grok_auth_mode {
                    AuthMode::ApiKey => "api_key",
                    AuthMode::OAuth => "oauth",
                },
                "auth_file": self.config
                    .grok_auth_file
                    .clone()
                    .or_else(crate::config::auth_path)
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unavailable".to_string()),
                "web_search_enabled": self.config.web_search_enabled,
                "x_search_enabled": ai_x_search_enabled,
                "reachable": grok_probe.ok,
                "detail": grok_probe.detail,
            },
            "tavily": {
                "api_url": self.config.tavily_api_url,
                "enabled": self.config.tavily_enabled,
                "reachable": tavily_probe.ok,
                "detail": tavily_probe.detail,
            },
            "exa": {
                "api_url": self.config.exa_api_url,
                "enabled": self.config.exa_enabled,
                "reachable": exa_probe.ok,
                "detail": exa_probe.detail,
            },
            "tinyfish": {
                "search_api_url": self.config.tinyfish_search_api_url,
                "fetch_api_url": self.config.tinyfish_fetch_api_url,
                "enabled": self.config.tinyfish_enabled,
                "reachable": tinyfish_probe.ok,
                "detail": tinyfish_probe.detail,
            },
            "firecrawl": {
                "api_url": self.config.firecrawl_api_url,
                "enabled": self.config.firecrawl_enabled,
                "reachable": firecrawl_probe.ok,
                "detail": firecrawl_probe.detail,
            },
            "source_chain": source_chain,
            "default_extra_sources": self.config.default_extra_sources,
            "fallback_sources": self.config.fallback_sources,
            "cache_size": self.config.cache_size,
            "timeout_seconds": self.config.timeout.as_secs(),
            "github_token": self.config.github_token_status(),
            "redacted": self.config.redacted_diagnostics()
        })
    }

    /// Probe one chain provider by name: live search for an active slot, a
    /// skip marker naming the missing key otherwise.
    async fn probe_chain_source(&self, name: &str) -> Probe {
        let active = self.source_slots.iter().find_map(|slot| match slot {
            SourceSlot::Active(entry) if entry.spec.name == name => Some(entry.clone()),
            _ => None,
        });
        match active {
            Some(entry) => probe_source(entry.provider.as_ref(), "https://example.com").await,
            None => {
                let key_var = CANONICAL_SOURCE_ORDER
                    .iter()
                    .find(|spec| spec.name == name)
                    .map(|spec| spec.key_var)
                    .unwrap_or("API key");
                Probe::skipped(format!("{key_var} not configured"))
            }
        }
    }

    async fn probe_grok(&self) -> Probe {
        // Mirror the real search shape so the probe doesn't fail the
        // adapter's "web_search tool intent" pre-check.
        let mut tools = Vec::new();
        if self.config.web_search_enabled {
            tools.push(SearchTool::web_search());
        }
        let request = SearchRequest {
            model: self.default_model.clone(),
            system: None,
            messages: vec![SearchMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::text("ping")],
            }],
            tools,
        };
        match self.ai.search(&request).await {
            Ok(_) => Probe::ok("grok responded"),
            Err(err) => Probe::failed(err.to_string()),
        }
    }

    fn build_search_request(
        &self,
        input: &WebSearchInput,
        extra_sources: &[Source],
    ) -> SearchRequest {
        let mut content = input.query.clone();
        if let Some(platform) = input.platform.as_deref().filter(|value| !value.is_empty()) {
            content.push_str("\n\nFocus platform: ");
            content.push_str(platform);
        }
        if let Some(days) = input.recency_days {
            content.push_str(&format!(
                "\n\nRestrict evidence to sources published within the last {days} day(s)."
            ));
        }
        if !input.include_domains.is_empty() {
            content.push_str("\n\nPrefer sources from: ");
            content.push_str(&input.include_domains.join(", "));
        }
        if !input.exclude_domains.is_empty() {
            content.push_str("\n\nDo not cite sources from: ");
            content.push_str(&input.exclude_domains.join(", "));
        }
        if !extra_sources.is_empty() {
            content.push_str("\n\nAdditional sources:\n");
            for source in extra_sources {
                content.push_str("- ");
                content.push_str(&source.url);
                if let Some(title) = &source.title {
                    content.push_str(" | ");
                    content.push_str(title);
                }
                content.push('\n');
            }
        }

        SearchRequest {
            model: input
                .model
                .clone()
                .unwrap_or_else(|| self.default_model.clone()),
            system: Some("Answer concisely with factual claims grounded in web search sources. Prefer primary sources. If sources are weak or unavailable, say so.".to_string()),
            messages: vec![SearchMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::text(content)],
            }],
            tools: vec![SearchTool::web_search()],
        }
    }
}

/// Outcome of the speculative source fan-out: the sources, the name of the
/// chain provider that served them (`None` when nothing answered), plus a
/// per-provider account of what happened. `notes` stays empty on the
/// first-provider happy path; earlier dead ends are recorded so a request
/// that ends with zero sources can say why.
struct RawSources {
    sources: Vec<Source>,
    origin: Option<&'static str>,
    notes: Vec<SourceNote>,
}

impl RawSources {
    fn found(sources: Vec<Source>, origin: Option<&'static str>) -> Self {
        Self {
            sources,
            origin,
            notes: Vec::new(),
        }
    }

    fn empty(notes: Vec<SourceNote>) -> Self {
        Self {
            sources: Vec::new(),
            origin: None,
            notes,
        }
    }
}

/// Upstream error text is quoted verbatim by the provider layer, and a gateway
/// or intermediary can answer with an arbitrarily large HTML page. Notes now
/// reach the caller inside a tool error, which no response budget applies to,
/// so each one is capped where it is recorded.
const MAX_SOURCE_NOTE_CHARS: usize = 300;

/// What kind of dead end a provider hit, which is what decides the useful
/// remedy. Collapsing these into "did it work" would hand every failure the
/// same advice, and the advice differs sharply: a deliberately disabled
/// fan-out is not a broken upstream, and neither is an honest empty result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteKind {
    /// The provider answered normally and simply had no matches, or was
    /// deliberately skipped by the filter gate. The query is the thing to
    /// change.
    NoResults,
    /// The operator switched this path off — a disabled provider, or a source
    /// budget of zero. Nothing is broken; the configuration says no.
    Config,
    /// A missing credential or an upstream failure. Something has to be fixed
    /// before this path can work at all.
    Broken,
}

/// One provider's account of why it produced nothing.
struct SourceNote {
    kind: NoteKind,
    detail: String,
}

impl SourceNote {
    fn new(kind: NoteKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: truncate_note(detail.into()),
        }
    }

    fn no_results(detail: impl Into<String>) -> Self {
        Self::new(NoteKind::NoResults, detail)
    }

    fn config(detail: impl Into<String>) -> Self {
        Self::new(NoteKind::Config, detail)
    }

    fn broken(detail: impl Into<String>) -> Self {
        Self::new(NoteKind::Broken, detail)
    }
}

fn truncate_note(mut detail: String) -> String {
    if detail.chars().count() <= MAX_SOURCE_NOTE_CHARS {
        return detail;
    }
    let cut = detail
        .char_indices()
        .nth(MAX_SOURCE_NOTE_CHARS)
        .map(|(idx, _)| idx)
        .unwrap_or(detail.len());
    detail.truncate(cut);
    detail.push('…');
    detail
}

/// One-line account of why no source provider produced anything, for the
/// zero-source failure message. Carries provider error text (status codes,
/// upstream detail) but never any credential: keys travel in headers, and the
/// provider errors these notes wrap quote only the endpoint and response body.
fn source_diagnosis(notes: &[SourceNote]) -> String {
    if notes.is_empty() {
        "no source provider was consulted".to_string()
    } else {
        notes
            .iter()
            .map(|note| note.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Keep only what the rest of the pipeline can act on, or explain what the
/// provider did instead.
///
/// Both normalizers accept an entry whose `url` is an empty string, and
/// `merge_sources` discards those — but only far downstream, after the caller
/// has already truncated to its source budget. Left in place, a blank entry
/// consumes a budget slot and silently displaces real evidence: a budget of 1
/// against `[blank, valid]` used to keep the blank, drop the valid source in
/// `merge_sources`, and fail the whole request. Filtering here means truncation
/// only ever applies to sources that will survive.
fn usable_or_note(
    provider: &str,
    sources: Vec<Source>,
) -> std::result::Result<Vec<Source>, SourceNote> {
    let returned = sources.len();
    let usable: Vec<Source> = sources
        .into_iter()
        .filter(|source| !source.url.trim().is_empty())
        .collect();
    if !usable.is_empty() {
        return Ok(usable);
    }
    // An empty answer is an honest no-match; an answer made entirely of blanks
    // is a provider returning something unusable, which is a different problem
    // with a different remedy.
    Err(if returned == 0 {
        SourceNote::no_results(format!("{provider}: no results"))
    } else {
        SourceNote::broken(format!("{provider}: results carried no usable URLs"))
    })
}

/// What would actually change the outcome, composed from every kind of dead
/// end present rather than one chosen by precedence. Mixed outcomes are the
/// common case — Tavily answering normally with no matches while Firecrawl has
/// no key — and there a different query can still succeed through the healthy
/// provider without anyone touching the other one's credentials. Suppressing
/// that advice would rule out the move that works; equally, the "do not just
/// repeat this" signal has to survive whenever nothing answered normally,
/// since that is what a client model needs to stop retrying a dead path.
fn fallback_remediation(notes: &[SourceNote]) -> String {
    if notes.is_empty() {
        return "Retrying will not help until the Grok upstream or the source-provider credentials are fixed.".to_string();
    }
    let has = |kind: NoteKind| notes.iter().any(|note| note.kind == kind);
    let mut parts = Vec::new();
    if has(NoteKind::NoResults) {
        parts.push("A source provider answered normally with no matches, so a different query or looser filters may help.");
    }
    if has(NoteKind::Config) {
        // Only a provider disabled through its own switch reaches this far: the
        // other producer of `Config` is the zero fan-out, which cannot happen
        // without a zero fallback budget, and `finalize_fallback` reports that
        // before asking for a remediation. Raising the budget would not
        // instantiate a switched-off provider, so it is not offered here.
        parts.push(
            "A source provider is switched off; enable it (TAVILY_ENABLED / EXA_ENABLED / TINYFISH_ENABLED / FIRECRAWL_ENABLED).",
        );
    }
    if has(NoteKind::Broken) {
        parts.push("A source provider is unusable until its credentials or upstream are fixed.");
    }
    if !has(NoteKind::NoResults) {
        // Deliberately not an absolute claim. A Grok timeout or 5xx clears on
        // its own, and an unchanged retry does succeed once it does — but not
        // before, which is what a client model looping on the same dead path
        // needs to hear.
        parts.push("Repeating this query unchanged will fail the same way until the condition above clears.");
    }
    parts.join(" ")
}

/// Why a source provider is absent from the service: the operator switched it
/// off (a configuration choice), or no key ever reached the process (something
/// to fix). The key hint names both channels because the HTTP transport
/// ignores server-side env keys entirely — a self-hoster who set the key in
/// their compose file needs to be told that the request header is the only one
/// that counts there.
fn unavailable_note(
    provider: &str,
    enabled: bool,
    enable_var: &str,
    key_var: &str,
    header: &str,
) -> SourceNote {
    if enabled {
        SourceNote::broken(format!(
            "{provider}: no API key ({key_var} for stdio, {header} header for the HTTP transport)"
        ))
    } else {
        SourceNote::config(format!("{provider}: disabled via {enable_var}"))
    }
}

#[cfg(test)]
mod remediation_tests {
    use super::*;

    // A switched-off provider is the only `Config` note that can reach the
    // remediation — the zero fan-out is reported by `finalize_fallback` before
    // this runs — and raising the source budget cannot instantiate one, so that
    // advice must not appear.
    #[test]
    fn disabled_provider_is_told_to_enable_not_to_raise_the_budget() {
        let notes = vec![unavailable_note(
            "tavily",
            false,
            "TAVILY_ENABLED",
            "TAVILY_API_KEY",
            "x-tavily-api-key",
        )];
        let remediation = fallback_remediation(&notes);
        assert!(remediation.contains("TAVILY_ENABLED"), "{remediation}");
        assert!(
            !remediation.contains("GROK_SEARCH_FALLBACK_SOURCES"),
            "{remediation}"
        );
    }

    // A Grok timeout or 5xx clears on its own, so the no-retry line must stop
    // short of claiming a repeat can never work — while still telling a looping
    // client that repeating it *now* changes nothing.
    #[test]
    fn retry_guidance_is_conditional_not_absolute() {
        let notes = vec![SourceNote::broken("tavily: provider error: HTTP 500")];
        let remediation = fallback_remediation(&notes);
        assert!(
            remediation.contains("until the condition above clears"),
            "{remediation}"
        );
        assert!(!remediation.contains("will not help"), "{remediation}");
    }

    // A provider that answered normally leaves a reformulated query on the
    // table, so the no-retry line stays out of the way entirely.
    #[test]
    fn a_healthy_empty_result_keeps_the_retry_door_open() {
        let notes = vec![
            SourceNote::no_results("tavily: no results"),
            SourceNote::broken("firecrawl: no API key"),
        ];
        let remediation = fallback_remediation(&notes);
        assert!(
            remediation.contains("a different query or looser filters"),
            "{remediation}"
        );
        assert!(
            !remediation.contains("Repeating this query unchanged"),
            "{remediation}"
        );
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    fn http() -> reqwest::Client {
        crate::providers::http::build_client(std::time::Duration::from_secs(5))
    }

    fn slot_names(slots: &[SourceSlot]) -> Vec<String> {
        slots
            .iter()
            .map(|slot| match slot {
                SourceSlot::Active(entry) => format!("active:{}", entry.spec.name),
                SourceSlot::Missing { spec, .. } => format!("missing:{}", spec.name),
            })
            .collect()
    }

    #[test]
    fn default_chain_orders_all_configured_providers_canonically() {
        let config = Config::from_env_map([
            ("TAVILY_API_KEY", "t"),
            ("EXA_API_KEY", "e"),
            ("TINYFISH_API_KEY", "f"),
            ("FIRECRAWL_API_KEY", "c"),
        ]);
        let slots = build_source_slots(&config, &http()).expect("slots");
        assert_eq!(
            slot_names(&slots),
            [
                "active:tavily",
                "active:exa",
                "active:tinyfish",
                "active:firecrawl"
            ]
        );
    }

    // Core providers keep their historical place in zero-source diagnostics
    // even when unconfigured; optional ones stay out unless named explicitly,
    // so "I never set up Exa" cannot read as a broken credential.
    #[test]
    fn default_chain_slots_missing_core_but_omits_missing_optional() {
        let config = Config::from_env_map([("TINYFISH_API_KEY", "f")]);
        let slots = build_source_slots(&config, &http()).expect("slots");
        assert_eq!(
            slot_names(&slots),
            ["missing:tavily", "active:tinyfish", "missing:firecrawl"]
        );
    }

    #[test]
    fn explicit_order_overrides_and_slots_every_named_provider() {
        let config = Config::from_env_map([
            ("TINYFISH_API_KEY", "f"),
            ("GROK_SEARCH_SOURCE_PROVIDERS", "tinyfish, Exa"),
        ]);
        let slots = build_source_slots(&config, &http()).expect("slots");
        assert_eq!(slot_names(&slots), ["active:tinyfish", "missing:exa"]);
    }

    #[test]
    fn unknown_provider_name_fails_construction() {
        let config = Config::from_env_map([("GROK_SEARCH_SOURCE_PROVIDERS", "tavily,serpapi")]);
        let err = match build_source_slots(&config, &http()) {
            Err(err) => err,
            Ok(_) => panic!("must reject unknown provider name"),
        };
        assert!(err.to_string().contains("serpapi"), "{err}");
    }

    #[test]
    fn labels_follow_the_serving_provider() {
        assert_eq!(enrichment_label(Some("exa")), "exa_enrichment");
        assert_eq!(fallback_label(Some("tinyfish")), "tinyfish_fallback");
        assert_eq!(fallback_label(None), "tavily_fallback");
    }

    /// Search always answers with no results.
    struct EmptySearchProvider;
    #[async_trait]
    impl SourceProvider for EmptySearchProvider {
        async fn search_sources(
            &self,
            _query: &str,
            _max_results: usize,
            _filters: &SearchFilters,
        ) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
        async fn fetch(&self, url: &str) -> Result<FetchedPage> {
            Ok(FetchedPage::text(format!("empty provider fetch {url}")))
        }
        async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
    }

    /// Serves sources whose provider label is the provider's own name.
    struct NamedSourceProvider(&'static str);
    #[async_trait]
    impl SourceProvider for NamedSourceProvider {
        async fn search_sources(
            &self,
            _query: &str,
            max_results: usize,
            _filters: &SearchFilters,
        ) -> Result<Vec<Source>> {
            Ok((0..max_results)
                .map(|idx| Source::new(format!("https://{}.example/{idx}", self.0), self.0))
                .collect())
        }
        async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
            Ok(FetchedPage::text(format!("content from {}", self.0)))
        }
        async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
    }

    struct ErrAiProvider;
    #[async_trait]
    impl AiProvider for ErrAiProvider {
        async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
            Err(GrokSearchError::Provider("ai down".to_string()))
        }
    }

    fn chain_service(slots: Vec<SourceSlot>, ai: Arc<dyn AiProvider>) -> SearchService {
        let config = Config::from_env_map([("GROK_SEARCH_API_KEY", "fake")]);
        SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai,
            source_slots: Arc::new(slots),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(5)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        }
    }

    fn concise_input() -> WebSearchInput {
        WebSearchInput {
            query: "chain test".to_string(),
            // Opt out of inline content so no real HTTP enrichment runs.
            include_content: Some(false),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn chain_falls_through_to_next_provider_and_labels_fallback_by_name() {
        let svc = chain_service(
            vec![
                SourceSlot::Active(SourceEntry {
                    spec: &TAVILY_SPEC,
                    provider: Arc::new(EmptySearchProvider),
                }),
                SourceSlot::Active(SourceEntry {
                    spec: &TINYFISH_SPEC,
                    provider: Arc::new(NamedSourceProvider("tinyfish")),
                }),
            ],
            Arc::new(ErrAiProvider),
        );
        let out = svc
            .web_search(concise_input())
            .await
            .expect("fallback output");
        assert!(out.fallback_used);
        assert!(!out.sources.is_empty());
        assert!(
            out.sources
                .iter()
                .all(|source| source.provider == "tinyfish_fallback"),
            "expected tinyfish_fallback labels, got: {:?}",
            out.sources
                .iter()
                .map(|source| source.provider.clone())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn filtered_requests_skip_filter_blind_providers_but_use_capable_ones() {
        // Firecrawl sits FIRST here yet must be skipped for a filtered
        // request; TinyFish (filter-capable) serves instead.
        let svc = chain_service(
            vec![
                SourceSlot::Active(SourceEntry {
                    spec: &FIRECRAWL_SPEC,
                    provider: Arc::new(NamedSourceProvider("firecrawl")),
                }),
                SourceSlot::Active(SourceEntry {
                    spec: &TINYFISH_SPEC,
                    provider: Arc::new(NamedSourceProvider("tinyfish")),
                }),
            ],
            Arc::new(ErrAiProvider),
        );
        let mut input = concise_input();
        input.include_domains = vec!["example.com".to_string()];
        let out = svc.web_search(input).await.expect("fallback output");
        assert!(
            out.sources
                .iter()
                .all(|source| source.provider == "tinyfish_fallback"),
            "filter-blind firecrawl must not serve a filtered request: {:?}",
            out.sources
                .iter()
                .map(|source| source.provider.clone())
                .collect::<Vec<_>>()
        );
    }

    /// Search hangs far past any test deadline; fetch/map are instant.
    struct HangingSearchProvider;
    #[async_trait]
    impl SourceProvider for HangingSearchProvider {
        async fn search_sources(
            &self,
            _query: &str,
            _max_results: usize,
            _filters: &SearchFilters,
        ) -> Result<Vec<Source>> {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Ok(Vec::new())
        }
        async fn fetch(&self, url: &str) -> Result<FetchedPage> {
            Ok(FetchedPage::text(format!("hanging provider fetch {url}")))
        }
        async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
    }

    // D-02: a hanging chain provider must be cut at the request's global
    // deadline, not granted a fresh full client timeout per chain position.
    #[tokio::test]
    async fn chain_is_bounded_by_the_global_deadline() {
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "fake"),
            ("GROK_SEARCH_TIMEOUT_SECONDS", "1"),
        ]);
        let svc = SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(ErrAiProvider),
            source_slots: Arc::new(vec![
                SourceSlot::Active(SourceEntry {
                    spec: &TAVILY_SPEC,
                    provider: Arc::new(HangingSearchProvider),
                }),
                SourceSlot::Active(SourceEntry {
                    spec: &TINYFISH_SPEC,
                    provider: Arc::new(NamedSourceProvider("tinyfish")),
                }),
            ]),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(5)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        };
        let started = std::time::Instant::now();
        let err = svc
            .web_search(concise_input())
            .await
            .expect_err("deadline-cut chain with a failed AI yields zero sources");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "chain must be cut at the ~1s deadline, took {:?}",
            started.elapsed()
        );
        assert!(
            err.to_string().contains("request deadline reached"),
            "error must name the deadline: {err}"
        );
    }

    /// Generic fetch hangs; search is instant and empty.
    struct HangingFetchProvider;
    #[async_trait]
    impl SourceProvider for HangingFetchProvider {
        async fn search_sources(
            &self,
            _query: &str,
            _max_results: usize,
            _filters: &SearchFilters,
        ) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
        async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Ok(FetchedPage::text("never reached"))
        }
        async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
    }

    // D-02 on the direct web_fetch path: two hanging chain providers must
    // share one request budget, not take a full client timeout each.
    #[tokio::test]
    async fn web_fetch_chain_is_bounded_by_one_deadline() {
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "fake"),
            ("GROK_SEARCH_TIMEOUT_SECONDS", "1"),
        ]);
        let svc = SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(vec![
                SourceSlot::Active(SourceEntry {
                    spec: &TAVILY_SPEC,
                    provider: Arc::new(HangingFetchProvider),
                }),
                SourceSlot::Active(SourceEntry {
                    spec: &FIRECRAWL_SPEC,
                    provider: Arc::new(HangingFetchProvider),
                }),
            ]),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(5)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        };
        let started = std::time::Instant::now();
        let err = svc
            .web_fetch("https://example.com/page", None)
            .await
            .expect_err("hanging chain must time out");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "web_fetch must be cut at the ~1s deadline, took {:?}",
            started.elapsed()
        );
        assert!(
            matches!(err, GrokSearchError::Timeout(_)),
            "expected a Timeout error, got: {err:?}"
        );
    }

    // web_map is a dedicated Tavily capability: excluding Tavily from the
    // supplemental chain must not break map while TAVILY_API_KEY is set.
    #[tokio::test]
    async fn web_map_survives_a_chain_that_excludes_tavily() {
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "fake"),
            ("TAVILY_API_KEY", "fake-tavily"),
            ("GROK_SEARCH_SOURCE_PROVIDERS", "tinyfish"),
        ]);
        let svc = SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(vec![SourceSlot::Active(SourceEntry {
                spec: &TINYFISH_SPEC,
                provider: Arc::new(NamedSourceProvider("tinyfish")),
            })]),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(5)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        };
        // The chain has no map-capable slot, so web_map instantiates Tavily
        // from config; the fake key means the call itself fails upstream, but
        // it must NOT fail as MissingConfig("TAVILY_API_KEY").
        let err = svc
            .web_map("https://example.com", 3)
            .await
            .expect_err("fake key cannot reach real Tavily");
        assert!(
            !matches!(err, GrokSearchError::MissingConfig(_)),
            "web_map must not report missing config when TAVILY_API_KEY is set: {err:?}"
        );
    }

    #[tokio::test]
    async fn enrichment_sources_carry_the_serving_providers_label() {
        // Grok succeeds (FakeAiProvider is verifiable); the supplemental
        // sources come from the second chain slot after the first is empty,
        // and must be labeled {provider}_enrichment.
        let svc = chain_service(
            vec![
                SourceSlot::Active(SourceEntry {
                    spec: &TAVILY_SPEC,
                    provider: Arc::new(EmptySearchProvider),
                }),
                SourceSlot::Active(SourceEntry {
                    spec: &EXA_SPEC,
                    provider: Arc::new(NamedSourceProvider("exa")),
                }),
            ],
            Arc::new(FakeAiProvider),
        );
        let out = svc
            .web_search(concise_input())
            .await
            .expect("search output");
        assert!(!out.fallback_used);
        assert!(
            out.sources
                .iter()
                .any(|source| source.provider == "exa_enrichment"),
            "expected exa_enrichment labels, got: {:?}",
            out.sources
                .iter()
                .map(|source| source.provider.clone())
                .collect::<Vec<_>>()
        );
    }
}

/// Pick the model the active transport actually understands. Responses speaks
/// Grok-native model names (`grok_model`); the chat-completions gateway speaks
/// whatever `OPENAI_COMPATIBLE_MODEL` declares, falling back to `grok_model`
/// only when the operator hasn't set one. Resolved once at service
/// construction so every outgoing `SearchRequest` carries the right default
/// — preventing the chat path from silently shipping a Grok-only ID.
fn resolve_default_model(config: &Config) -> String {
    use crate::config::Transport;
    match config.transport {
        Transport::Responses => config.grok_model.clone(),
        Transport::ChatCompletions => config
            .openai_compatible_model
            .clone()
            .unwrap_or_else(|| config.grok_model.clone()),
    }
}

/// Per-source label for the enrichment path: the serving chain provider's
/// name plus an `_enrichment` suffix ("tavily_enrichment", "exa_enrichment",
/// …). `None` (nothing served) keeps the historical "tavily" default, though
/// it only ever labels an empty list.
fn enrichment_label(origin: Option<&'static str>) -> String {
    format!("{}_enrichment", origin.unwrap_or("tavily"))
}

/// Per-source label for the degraded path, `{provider}_fallback` (#30).
fn fallback_label(origin: Option<&'static str>) -> String {
    format!("{}_fallback", origin.unwrap_or("tavily"))
}

/// Maps a failed Grok call to a stable `fallback_reason` identifier. Kept at
/// enum-variant granularity on purpose: distinguishing timeout / auth / parse
/// from a generic provider failure is the diagnostically useful axis, while
/// sub-parsing HTTP status codes out of `Provider(String)` would be fragile.
/// `Provider` (and any other variant) preserves the legacy `grok_provider_error`.
fn grok_error_reason(err: &GrokSearchError) -> &'static str {
    match err {
        GrokSearchError::Timeout(_) => "grok_timeout",
        GrokSearchError::OAuth(_) => "grok_auth_error",
        GrokSearchError::Parse(_) => "grok_parse_error",
        _ => "grok_provider_error",
    }
}

fn grok_unverifiable_reason(response: &SearchResponse) -> Option<&'static str> {
    if response.content.trim().is_empty() {
        return Some("grok_content_empty");
    }
    if response.sources.is_empty() {
        return Some("grok_sources_empty");
    }
    None
}

fn apply_fetch_limit(
    url: &str,
    mut content: String,
    max_chars: Option<usize>,
    source_type: crate::sources::SourceType,
    fallback_reason: Option<String>,
) -> WebFetchOutput {
    let Some(limit) = max_chars else {
        let original_length = content.chars().count();
        return WebFetchOutput {
            url: url.to_string(),
            content,
            original_length,
            truncated: false,
            source_type,
            fallback_reason,
        };
    };

    let mut count = 0usize;
    let mut cutoff: Option<usize> = None;
    for (byte_idx, _) in content.char_indices() {
        if count == limit {
            cutoff = Some(byte_idx);
            break;
        }
        count += 1;
    }

    match cutoff {
        Some(byte_idx) => {
            let extra = content[byte_idx..].chars().count();
            content.truncate(byte_idx);
            WebFetchOutput {
                url: url.to_string(),
                content,
                original_length: limit + extra,
                truncated: true,
                source_type,
                fallback_reason,
            }
        }
        None => WebFetchOutput {
            url: url.to_string(),
            content,
            original_length: count,
            truncated: false,
            source_type,
            fallback_reason,
        },
    }
}

/// Generic (non-specialist) content fetch via the configured source-provider
/// chain, first non-empty page wins. Shared by `web_fetch` and inline
/// enrichment so both agree on how an ordinary URL is retrieved once no
/// specialist extractor matches. Returns `MissingConfig` only when no
/// provider is configured at all; otherwise the last attempt's real error
/// surfaces, so users are not sent to debug config that is actually set.
async fn generic_source_fetch(chain: &[SourceEntry], url: &str) -> Result<FetchedPage> {
    let mut last_error: Option<GrokSearchError> = None;
    for entry in chain {
        match entry.provider.fetch(url).await {
            Ok(page) if !page.content.trim().is_empty() => return Ok(page),
            Ok(_) => {
                last_error = Some(GrokSearchError::Provider(format!(
                    "{} returned empty content for {url}",
                    entry.spec.display
                )))
            }
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or(GrokSearchError::MissingConfig(SOURCE_PROVIDER_KEYS)))
}

/// The keys that make a generic fetch possible at all. Named from one place so
/// the `web_fetch` error and the inline-enrichment note cannot drift apart.
const SOURCE_PROVIDER_KEYS: &str =
    "TAVILY_API_KEY, EXA_API_KEY, TINYFISH_API_KEY or FIRECRAWL_API_KEY";

/// Why an ordinary URL could not be enriched.
///
/// With an empty chain there is no source provider to fetch it, and that has
/// nothing to do with specialist extractors: those need no key and simply did
/// not match this URL. Reporting the specialist outcome here sends the reader
/// off to debug the wrong subsystem — it is how the reporter of issue #39 came
/// to believe specialists require API keys.
fn enrichment_failure_reason(chain: &[SourceEntry], specialist_reason: &str) -> String {
    if chain.is_empty() {
        format!("no source provider configured (set {SOURCE_PROVIDER_KEYS})")
    } else {
        specialist_reason.to_string()
    }
}

/// One enrichment outcome: the content to store plus any metadata backfill
/// harvested from the fetched page. Failure notes never carry metadata.
struct EnrichedFetch {
    content: Option<String>,
    title: Option<String>,
    published_date: Option<String>,
}

impl EnrichedFetch {
    /// A deterministic failure note stored as content — never a title source.
    fn note(note: String) -> Self {
        Self {
            content: Some(note),
            title: None,
            published_date: None,
        }
    }

    /// Specialist markdown: heading fallback only (specialist extractors
    /// return no structured metadata). Heading extraction runs before
    /// truncation so a tight `max_chars` cannot cut the title line in half.
    fn from_markdown(md: String, max_chars: usize) -> Self {
        let title = first_markdown_heading(&md);
        Self {
            content: Some(md.chars().take(max_chars).collect()),
            title,
            published_date: None,
        }
    }

    /// Generic provider page: provider metadata first, heading as fallback.
    fn from_page(page: FetchedPage, max_chars: usize) -> Self {
        let title = page
            .title
            .filter(|title| !is_junk_title(title))
            .or_else(|| first_markdown_heading(&page.content));
        Self {
            content: Some(page.content.chars().take(max_chars).collect()),
            title,
            published_date: page.published_date,
        }
    }
}

/// Headings longer than this are prose that happens to start with `#`, not a
/// plausible page title.
const MAX_HEADING_TITLE_CHARS: usize = 200;

/// First ATX heading (`# ` … `###### `) in the fetched markdown, as a
/// title-of-last-resort for sources whose provider returned none. Only the
/// first heading is considered — if it is junk or oversized, guessing a later
/// section heading would mislabel the page, so the title stays `None`.
fn first_markdown_heading(markdown: &str) -> Option<String> {
    let heading = markdown.lines().find_map(|line| {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if hashes == 0 || hashes > 6 {
            return None;
        }
        // ATX requires whitespace after the marker run; "#hashtag" is prose.
        let rest = &trimmed[hashes..];
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let text = rest.trim();
        (!text.is_empty()).then(|| text.to_string())
    })?;
    (heading.chars().count() <= MAX_HEADING_TITLE_CHARS && !is_junk_title(&heading))
        .then_some(heading)
}

/// Concurrently back-fill `Source.content` for the first `max_sources` sources
/// via the Phase 1 `resolve_content` pipeline; later sources stay
/// metadata-only (content = None) so a Grok response with dozens of citations
/// cannot blow up the payload — agents drill into them with `web_fetch`.
/// Bounded by `concurrency` (Semaphore) and the shared `deadline` (D-02:
/// per-source `timeout_at`, not an independent budget). Every enriched source
/// ends with `content = Some(..)` — real markdown (truncated to `max_chars`)
/// on success, or a deterministic `_Failed to retrieve: ..._` note on any
/// failure/timeout/invalid-url (D-05 within the inline window: never None,
/// never empty). Source order is preserved. While content is in hand, missing
/// `title`/`published_date` are back-filled from the fetched page (issue #21).
#[allow(clippy::too_many_arguments)]
async fn enrich_sources(
    sources: Vec<Source>,
    deadline: tokio::time::Instant,
    client: &reqwest::Client,
    router: &Arc<crate::sources::SourceRouter>,
    caps: crate::sources::SourceCaps,
    concurrency: usize,
    max_chars: usize,
    max_sources: usize,
    chain: Vec<SourceEntry>,
) -> Vec<Source> {
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut set: tokio::task::JoinSet<(usize, EnrichedFetch)> = tokio::task::JoinSet::new();

    for (idx, source) in sources.iter().enumerate().take(max_sources) {
        let permit = Arc::clone(&sem);
        let url_str = source.url.clone();
        let client = client.clone();
        let router = Arc::clone(router);
        let caps = caps.clone();
        let chain = chain.clone();

        set.spawn(async move {
            // acquire is micro-second scale for concurrency<=5; deadline
            // enforcement applies to the resolve_content call itself.
            let _permit = permit.acquire_owned().await.ok();
            let fetched = match url::Url::parse(&url_str) {
                Err(_) => EnrichedFetch::note(format!(
                    "_Failed to retrieve: invalid_url_\n\nSource: {url_str}"
                )),
                Ok(parsed) => {
                    let future = crate::sources::resolve_content(&client, &parsed, &router, &caps);
                    match tokio::time::timeout_at(deadline, future).await {
                        Ok(Ok((md, _kind))) => EnrichedFetch::from_markdown(md, max_chars),
                        // Specialist produced no content — either no specialist
                        // matched (generic URL) OR a matched specialist's API
                        // failed/rate-limited/rendered empty. Either way, mirror
                        // web_fetch and try the configured Tavily/Firecrawl generic
                        // fetch before giving up, so inline content still has page
                        // evidence when a source provider can fetch the URL (P1 +
                        // specialist-failure fallback). The original `reason` is
                        // surfaced only if the generic fetch also fails, and only
                        // when there was a chain to fail — see
                        // `enrichment_failure_reason`.
                        Ok(Err(reason)) => {
                            let generic = generic_source_fetch(&chain, &url_str);
                            match tokio::time::timeout_at(deadline, generic).await {
                                Ok(Ok(page)) => EnrichedFetch::from_page(page, max_chars),
                                Ok(Err(_)) => EnrichedFetch::note(format!(
                                    "_Failed to retrieve: {}_\n\nSource: {url_str}",
                                    enrichment_failure_reason(&chain, &reason)
                                )),
                                Err(_elapsed) => EnrichedFetch::note(format!(
                                    "_Failed to retrieve: timeout_\n\nSource: {url_str}"
                                )),
                            }
                        }
                        Err(_elapsed) => EnrichedFetch::note(format!(
                            "_Failed to retrieve: timeout_\n\nSource: {url_str}"
                        )),
                    }
                }
            };
            (idx, fetched)
        });
    }

    let mut results: Vec<(usize, EnrichedFetch)> = Vec::with_capacity(sources.len());
    while let Some(res) = set.join_next().await {
        if let Ok(pair) = res {
            results.push(pair);
        }
    }

    results.sort_by_key(|(idx, _)| *idx);
    let mut out = sources;
    for (idx, fetched) in results {
        let source = &mut out[idx];
        source.content = fetched.content;
        // Metadata backfill (issue #21): most Grok citations arrive as bare
        // URLs, so the fetched page is the only place a title/date can come
        // from. Fill only what upstream never provided — real upstream
        // metadata always wins, and un-enriched tail sources keep honest nulls.
        if source.title.is_none() {
            source.title = fetched.title;
        }
        if source.published_date.is_none() {
            source.published_date = fetched.published_date;
        }
    }
    out
}

/// Approximate serialized footprint of one source: every metadata field plus
/// inline content plus a fixed allowance for JSON keys/quotes/separators. The
/// budget must track what actually lands in the agent's context — a broad
/// query where Grok cites 50+ pages overflows on metadata alone, so counting
/// only inline content under-reports the payload.
fn source_weight(source: &Source) -> usize {
    const JSON_OVERHEAD: usize = 64;
    let opt_chars = |v: &Option<String>| v.as_deref().map(|s| s.chars().count()).unwrap_or(0);
    source.url.chars().count()
        + source.provider.chars().count()
        + opt_chars(&source.title)
        + opt_chars(&source.description)
        + opt_chars(&source.published_date)
        + source
            .content
            .as_deref()
            .map(|c| c.chars().count())
            .unwrap_or(0)
        + JSON_OVERHEAD
}

/// Trim the response from the TAIL until `answer_chars` plus the weighted
/// source list fits the `budget`. Head sources (Grok's own citations rank
/// first) survive intact. Two passes:
///
/// 1. Replace tail inline content with an actionable note naming `web_fetch`
///    and `get_sources` — the official MCP fetch server's "call again with
///    start_index" guidance, applied to sources.
/// 2. Still over budget (metadata overflow): drop whole tail sources from the
///    returned list, always keeping at least one.
///
/// The synthesized answer is never trimmed. Returns whether anything was
/// trimmed; callers always trim a clone so the session cache keeps everything.
fn apply_response_budget(
    answer_chars: usize,
    sources: &mut Vec<Source>,
    budget: usize,
    session_id: &str,
) -> bool {
    let content_chars = |s: &Source| s.content.as_deref().map(|c| c.chars().count()).unwrap_or(0);
    let mut total: usize = answer_chars + sources.iter().map(source_weight).sum::<usize>();
    if total <= budget {
        return false;
    }

    // Pass 1: swap tail inline content for recovery notes.
    for idx in (0..sources.len()).rev() {
        if total <= budget {
            break;
        }
        let len = content_chars(&sources[idx]);
        if len == 0 {
            continue;
        }
        let url = sources[idx].url.clone();
        let note = |verb: &str| {
            format!(
                "_[{verb}: response budget reached — full text via web_fetch(\"{url}\") or get_sources(session_id=\"{session_id}\", offset={idx}, limit=1)]_"
            )
        };
        let omit_note = note("inline content omitted");
        let omit_len = omit_note.chars().count();
        if len <= omit_len {
            // Replacing would not shrink the payload; leave it alone.
            continue;
        }
        let overshoot = total - budget;
        let trim_note = note("truncated");
        // "\n\n" separator + note must fit inside the chars we reclaim.
        let trim_overhead = trim_note.chars().count() + 2;
        if len > overshoot + trim_overhead {
            // Partial trim: keep a prefix so the head of the document survives.
            let keep = len - overshoot - trim_overhead;
            let prefix: String = sources[idx]
                .content
                .as_deref()
                .unwrap_or_default()
                .chars()
                .take(keep)
                .collect();
            sources[idx].content = Some(format!("{prefix}\n\n{trim_note}"));
            total -= overshoot;
        } else {
            sources[idx].content = Some(omit_note);
            total = total - len + omit_len;
        }
    }

    // Pass 2: metadata alone still over budget — cut whole tail sources.
    // They stay in the cache; get_sources(offset=..) pages through them.
    while total > budget && sources.len() > 1 {
        let dropped = sources.pop().expect("len > 1");
        total -= source_weight(&dropped);
    }

    true
}

fn with_provider(
    mut sources: Vec<Source>,
    provider: impl Into<std::borrow::Cow<'static, str>>,
) -> Vec<Source> {
    let provider = provider.into();
    for source in &mut sources {
        source.provider = provider.clone();
    }
    sources
}

struct Probe {
    ok: bool,
    detail: String,
}

impl Probe {
    fn ok(detail: impl Into<String>) -> Self {
        Self {
            ok: true,
            detail: detail.into(),
        }
    }
    fn failed(detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            detail: detail.into(),
        }
    }
    fn skipped(detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            detail: detail.into(),
        }
    }
}

async fn probe_source(provider: &dyn SourceProvider, sample_url: &str) -> Probe {
    // Use a short keyword search as a lightweight liveness signal.
    let filters = SearchFilters::default();
    match provider.search_sources("ping", 1, &filters).await {
        Ok(_) => Probe::ok(format!("reachable (sample probe via {sample_url} ok)")),
        Err(err) => Probe::failed(err.to_string()),
    }
}

struct FakeAiProvider;

#[async_trait]
impl AiProvider for FakeAiProvider {
    async fn search(&self, _request: &SearchRequest) -> Result<SearchResponse> {
        Ok(SearchResponse {
            content: "OpenAI published a verifiable update.".to_string(),
            sources: vec![
                Source::new("https://openai.com/news", "grok_responses").with_title("OpenAI News")
            ],
        })
    }
}

struct FakeSourceProvider;

#[async_trait]
impl SourceProvider for FakeSourceProvider {
    async fn search_sources(
        &self,
        _query: &str,
        max_results: usize,
        _filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        Ok((0..max_results)
            .map(|idx| {
                Source::new(format!("https://example.com/source-{idx}"), "tavily")
                    .with_title(format!("Source {idx}"))
            })
            .collect())
    }

    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        Ok(FetchedPage::text(format!("Fetched content from {url}")))
    }

    async fn map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        Ok((0..max_results)
            .map(|idx| Source::new(format!("{url}/page-{idx}"), "tavily"))
            .collect())
    }
}

#[cfg(test)]
mod transport_dispatch_tests {
    use super::*;
    use crate::config::Transport;

    #[test]
    fn service_constructs_for_chat_completions_transport() {
        let config = Config::from_env_map([
            ("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
            ("OPENAI_COMPATIBLE_MODEL", "grok-4.3-fast"),
            ("TAVILY_API_KEY", "fake-tavily"),
        ]);
        assert_eq!(config.transport, Transport::ChatCompletions);
        let svc = SearchService::new(config).expect("service should build");
        // Smoke: just ensure construction doesn't blow up. The actual provider
        // type is hidden behind Arc<dyn AiProvider>; we verify behavior in the
        // ignored e2e probe (Task 7) and adapter unit tests (Tasks 3-4).
        drop(svc);
    }

    #[test]
    fn service_rejects_chat_completions_without_url() {
        let config = Config::from_env_map([("OPENAI_COMPATIBLE_API_KEY", "sk-fake")]);
        // url missing -> falls back to Responses transport, which then needs
        // GROK_SEARCH_API_KEY which is also missing -> MissingConfig.
        assert!(SearchService::new(config).is_err());
    }

    #[test]
    fn default_model_follows_chat_completions_when_compat_model_set() {
        // Reproduces the regression: SearchService::build_search_request used
        // to stamp `grok_model` into every SearchRequest, masking
        // OPENAI_COMPATIBLE_MODEL on the chat-completions transport.
        let config = Config::from_env_map([
            ("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
            ("OPENAI_COMPATIBLE_MODEL", "gpt-4o-mini"),
            ("GROK_SEARCH_MODEL", "grok-4-1-fast-reasoning"),
        ]);
        assert_eq!(config.transport, Transport::ChatCompletions);
        assert_eq!(resolve_default_model(&config), "gpt-4o-mini");
    }

    #[test]
    fn default_model_falls_back_to_grok_model_when_compat_model_missing() {
        let config = Config::from_env_map([
            ("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
            ("GROK_SEARCH_MODEL", "grok-4-1-fast-reasoning"),
        ]);
        assert_eq!(config.transport, Transport::ChatCompletions);
        assert_eq!(resolve_default_model(&config), "grok-4-1-fast-reasoning");
    }

    #[test]
    fn default_model_uses_grok_model_on_responses_transport() {
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "xai-fake"),
            ("GROK_SEARCH_MODEL", "grok-4-1-fast-reasoning"),
            ("OPENAI_COMPATIBLE_MODEL", "gpt-4o-mini"),
        ]);
        assert_eq!(config.transport, Transport::Responses);
        assert_eq!(resolve_default_model(&config), "grok-4-1-fast-reasoning");
    }

    #[tokio::test]
    async fn doctor_reports_openai_compatible_transport_fields() {
        // Regression: doctor() used to hardcode "grok_responses" / grok_model /
        // grok_api_url, masking what the service actually dispatches to on the
        // chat-completions transport. Now it must reflect compat config.
        let config = Config::from_env_map([
            ("OPENAI_COMPATIBLE_API_URL", "https://compat.example/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
            ("OPENAI_COMPATIBLE_MODEL", "gpt-4o-mini"),
            ("GROK_SEARCH_MODEL", "grok-4-1-fast-reasoning"),
            // X-search is silently ignored on this transport — doctor must
            // report the effective behavior (false), not the raw env flag.
            ("GROK_SEARCH_X_SEARCH", "true"),
        ]);
        assert_eq!(config.transport, Transport::ChatCompletions);

        // Hand-build the service with fake AI to avoid any real HTTP from
        // probe_grok during doctor().
        let svc = SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(Vec::new()),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        };

        let report = svc.doctor().await;
        assert_eq!(report["provider"], "openai_compatible");
        assert_eq!(report["transport"], "openai_compatible");
        assert_eq!(report["grok"]["api_url"], "https://compat.example/v1");
        assert_eq!(report["grok"]["model"], "gpt-4o-mini");
        assert_eq!(report["grok"]["x_search_enabled"], false);
    }

    #[tokio::test]
    async fn doctor_still_reports_grok_responses_on_responses_transport() {
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "xai-fake"),
            ("GROK_SEARCH_MODEL", "grok-4-1-fast-reasoning"),
        ]);
        assert_eq!(config.transport, Transport::Responses);

        let svc = SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(Vec::new()),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        };

        let report = svc.doctor().await;
        assert_eq!(report["provider"], "grok_responses");
        assert_eq!(report["grok"]["model"], "grok-4-1-fast-reasoning");
    }

    #[tokio::test]
    async fn doctor_reports_github_token_status() {
        // With GITHUB_TOKEN set -> "set", and the raw value never leaks.
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "xai-fake"),
            ("GITHUB_TOKEN", "ghp_test"),
        ]);
        let svc = SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(Vec::new()),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        };
        let report = svc.doctor().await;
        assert_eq!(report["github_token"], "set");
        // No-leak: the full report must not contain the token value anywhere.
        assert!(
            !report.to_string().contains("ghp_test"),
            "token value leaked into doctor report: {report}"
        );

        // Without GITHUB_TOKEN -> "unset".
        let config_unset = Config::from_env_map([("GROK_SEARCH_API_KEY", "xai-fake")]);
        let svc_unset = SearchService {
            default_model: resolve_default_model(&config_unset),
            config: config_unset,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(Vec::new()),
            cache: Arc::new(Mutex::new(SourceCache::new(16))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        };
        let report_unset = svc_unset.doctor().await;
        assert_eq!(report_unset["github_token"], "unset");
    }

    #[tokio::test]
    async fn fake_with_router_constructs_and_clones() {
        let svc = SearchService::fake_with_router(
            Arc::new(FakeSourceProvider),
            None,
            crate::sources::SourceRouter::default(),
        );
        // SearchService derives Clone; storing Arc<SourceRouter> must preserve it.
        let _clone = svc.clone();
    }
}

#[cfg(test)]
mod enrich_tests {
    use super::*;
    use crate::sources::{SourceCaps, SourceExtractor, SourceRouter, SourceType};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use url::Url;

    /// Always-matching extractor that records peak concurrency and returns a
    /// fixed body after a visibility sleep.
    struct CountingExtractor {
        peak: Arc<AtomicUsize>,
        current: Arc<AtomicUsize>,
        sleep_ms: u64,
    }
    #[async_trait]
    impl SourceExtractor for CountingExtractor {
        fn matches(&self, _url: &Url) -> bool {
            true
        }
        fn kind(&self) -> SourceType {
            SourceType::Wikipedia
        }
        async fn fetch_render(
            &self,
            _c: &reqwest::Client,
            _u: &Url,
            _caps: &SourceCaps,
        ) -> Result<String> {
            let n = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(n, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(self.sleep_ms)).await;
            self.current.fetch_sub(1, Ordering::SeqCst);
            Ok("content".to_string())
        }
    }

    /// URL-discriminating failure extractor: matches ONLY urls containing
    /// `fail_url_marker`, so a router can route one source here and the rest to
    /// CountingExtractor (true fault isolation).
    struct MarkerErrExtractor {
        fail_url_marker: String,
    }
    #[async_trait]
    impl SourceExtractor for MarkerErrExtractor {
        fn matches(&self, url: &Url) -> bool {
            url.as_str().contains(&self.fail_url_marker)
        }
        fn kind(&self) -> SourceType {
            SourceType::GithubIssue
        }
        async fn fetch_render(
            &self,
            _c: &reqwest::Client,
            _u: &Url,
            _caps: &SourceCaps,
        ) -> Result<String> {
            Err(crate::error::GrokSearchError::Provider(
                "always_fails".to_string(),
            ))
        }
    }

    /// Returns an oversized body to exercise the per-source char cap.
    struct OversizeExtractor {
        len: usize,
    }
    #[async_trait]
    impl SourceExtractor for OversizeExtractor {
        fn matches(&self, _url: &Url) -> bool {
            true
        }
        fn kind(&self) -> SourceType {
            SourceType::Wikipedia
        }
        async fn fetch_render(
            &self,
            _c: &reqwest::Client,
            _u: &Url,
            _caps: &SourceCaps,
        ) -> Result<String> {
            Ok("x".repeat(self.len))
        }
    }

    /// Hangs far past any test deadline — used to trigger the timeout note.
    struct HangingExtractor;
    #[async_trait]
    impl SourceExtractor for HangingExtractor {
        fn matches(&self, _url: &Url) -> bool {
            true
        }
        fn kind(&self) -> SourceType {
            SourceType::Wikipedia
        }
        async fn fetch_render(
            &self,
            _c: &reqwest::Client,
            _u: &Url,
            _caps: &SourceCaps,
        ) -> Result<String> {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok("never".to_string())
        }
    }

    /// Supplemental provider whose `search_sources` returns example.com sources
    /// but whose generic `fetch` always errors — used to exercise the
    /// "specialist failed AND generic fetch failed → note" path.
    struct SearchOkFetchErrProvider;
    #[async_trait]
    impl SourceProvider for SearchOkFetchErrProvider {
        async fn search_sources(
            &self,
            _query: &str,
            max_results: usize,
            _filters: &SearchFilters,
        ) -> Result<Vec<Source>> {
            Ok((0..max_results)
                .map(|idx| Source::new(format!("https://example.com/source-{idx}"), "tavily"))
                .collect())
        }
        async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
            Err(crate::error::GrokSearchError::Provider(
                "generic fetch unavailable".to_string(),
            ))
        }
        async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
    }

    /// Generic `fetch` succeeds but yields whitespace-only content — exercises
    /// the "primary configured but empty" error path of `generic_source_fetch`.
    struct EmptyFetchProvider;
    #[async_trait]
    impl SourceProvider for EmptyFetchProvider {
        async fn search_sources(
            &self,
            _query: &str,
            _max_results: usize,
            _filters: &SearchFilters,
        ) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
        async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
            Ok(FetchedPage::text("  \n"))
        }
        async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
    }

    /// Build a SearchService with fake AI + a caller-supplied supplemental
    /// provider, router, and config. Mirrors the doctor_* struct-literal tests.
    fn service_with_sources(
        config: Config,
        router: SourceRouter,
        sources: Option<Arc<dyn SourceProvider>>,
    ) -> SearchService {
        let source_slots = sources
            .map(|provider| {
                vec![SourceSlot::Active(SourceEntry {
                    spec: &TAVILY_SPEC,
                    provider,
                })]
            })
            .unwrap_or_default();
        SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            source_slots: Arc::new(source_slots),
            cache: Arc::new(Mutex::new(SourceCache::new(64))),
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(router),
        }
    }

    fn service_with(config: Config, router: SourceRouter) -> SearchService {
        service_with_sources(config, router, Some(Arc::new(FakeSourceProvider)))
    }

    fn enrich_config() -> Config {
        Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "fake-grok"),
            ("TAVILY_API_KEY", "fake-tavily"),
        ])
    }

    fn base_input() -> WebSearchInput {
        WebSearchInput {
            query: "q".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn counting_extractor_self_test() {
        // Sanity: the helper itself records concurrency.
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let router = SourceRouter::with_extractors(vec![Box::new(CountingExtractor {
            peak: Arc::clone(&peak),
            current: Arc::clone(&current),
            sleep_ms: 5,
        })]);
        let svc = service_with(enrich_config(), router);
        let _ = svc.web_search(base_input()).await.expect("web_search");
        assert!(peak.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn web_search_inline_default_fills_content() {
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let router = SourceRouter::with_extractors(vec![Box::new(CountingExtractor {
            peak,
            current,
            sleep_ms: 0,
        })]);
        let svc = service_with(enrich_config(), router);
        let out = svc.web_search(base_input()).await.expect("web_search");

        assert!(!out.sources.is_empty());
        for s in &out.sources {
            let c = s.content.as_deref().unwrap_or("");
            assert!(!c.is_empty(), "every source must have non-empty content");
        }
    }

    #[tokio::test]
    async fn enrich_generic_url_uses_provider_fetch_fallback() {
        // No specialist matches the supplemental URLs → inline enrichment must
        // fall back to the configured source provider's generic fetch (mirroring
        // web_fetch), not emit a `_Failed to retrieve: no_specialist_match_`
        // note for ordinary search results (P1).
        let svc = service_with(enrich_config(), SourceRouter::default());
        let out = svc.web_search(base_input()).await.expect("web_search");

        assert!(!out.sources.is_empty());
        for s in &out.sources {
            let c = s.content.as_deref().unwrap_or("");
            assert!(
                c.starts_with("Fetched content from"),
                "generic source must use the provider fetch fallback, got: {c:?}"
            );
            assert!(
                !c.contains("no_specialist_match"),
                "must not leak the no_specialist_match note: {c:?}"
            );
        }
    }

    #[tokio::test]
    async fn enrich_names_the_missing_source_provider_when_the_chain_is_empty() {
        // With no source provider configured, nothing can fetch an ordinary
        // page — and that has nothing to do with specialist extractors, which
        // need no key and simply did not match this URL. Saying
        // "no_specialist_match" here sends the reader to debug the wrong thing;
        // it is what taught the reporter of #39 that specialists need keys.
        let svc = service_with_sources(enrich_config(), SourceRouter::default(), None);
        let out = svc.web_search(base_input()).await.expect("web_search");

        assert!(!out.sources.is_empty());
        for s in &out.sources {
            let c = s.content.as_deref().unwrap_or("");
            assert!(
                !c.contains("specialist"),
                "an empty chain is not a specialist problem: {c:?}"
            );
            assert!(
                c.contains("no source provider configured"),
                "the note must name what is actually missing: {c:?}"
            );
            assert!(
                c.contains("TAVILY_API_KEY"),
                "the note must say how to supply one: {c:?}"
            );
        }
    }

    #[tokio::test]
    async fn empty_chain_names_the_same_keys_to_web_fetch_and_to_enrichment() {
        // Two paths, one answer. They drifted apart before because each spelled
        // the missing config out by hand.
        let svc = service_with_sources(enrich_config(), SourceRouter::default(), None);

        let fetch_error = svc
            .web_fetch("https://example.com/plain", None)
            .await
            .expect_err("nothing can fetch a generic URL with an empty chain")
            .to_string();
        let out = svc.web_search(base_input()).await.expect("web_search");
        let note = out.sources[0].content.clone().unwrap_or_default();

        for key in [
            "TAVILY_API_KEY",
            "EXA_API_KEY",
            "TINYFISH_API_KEY",
            "FIRECRAWL_API_KEY",
        ] {
            assert!(
                fetch_error.contains(key),
                "web_fetch omits {key}: {fetch_error}"
            );
            assert!(note.contains(key), "enrichment omits {key}: {note}");
        }
    }

    #[tokio::test]
    async fn specialist_extractors_still_work_without_any_source_provider() {
        // The whole point of the distinction: specialists take no key, so an
        // empty chain must not stop them.
        let router = SourceRouter::with_extractors(vec![Box::new(CountingExtractor {
            peak: Arc::new(AtomicUsize::new(0)),
            current: Arc::new(AtomicUsize::new(0)),
            sleep_ms: 0,
        })]);
        let svc = service_with_sources(enrich_config(), router, None);
        let out = svc.web_search(base_input()).await.expect("web_search");

        assert!(!out.sources.is_empty());
        for s in &out.sources {
            let c = s.content.as_deref().unwrap_or("");
            assert!(
                !c.contains("Failed to retrieve"),
                "a specialist needs no source provider: {c:?}"
            );
            assert!(!c.is_empty(), "specialist content must land: {c:?}");
        }
    }

    #[tokio::test]
    async fn enrich_concurrency_is_bounded() {
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let router = SourceRouter::with_extractors(vec![Box::new(CountingExtractor {
            peak: Arc::clone(&peak),
            current: Arc::clone(&current),
            sleep_ms: 25, // wide enough window for overlap to register
        })]);
        let mut config = enrich_config();
        config.enrich_concurrency = 2;
        let svc = service_with(config, router);

        let _ = svc.web_search(base_input()).await.expect("web_search");
        // 4 sources, concurrency 2 → peak must never exceed 2.
        assert!(
            peak.load(Ordering::SeqCst) <= 2,
            "peak={}",
            peak.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn enrich_truncates_to_max_chars() {
        let router =
            SourceRouter::with_extractors(vec![Box::new(OversizeExtractor { len: 20_000 })]);
        let svc = service_with(enrich_config(), router); // default enrich_max_chars = 15000
        let out = svc.web_search(base_input()).await.expect("web_search");

        for s in &out.sources {
            let len = s.content.as_deref().map(|c| c.chars().count()).unwrap_or(0);
            assert!(len <= 15_000, "content len {len} exceeds cap");
            assert!(len > 0);
        }
    }

    #[tokio::test]
    async fn enrich_fault_isolation_one_fails_rest_ok() {
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let router = SourceRouter::with_extractors(vec![
            Box::new(MarkerErrExtractor {
                fail_url_marker: "openai.com".to_string(),
            }),
            Box::new(CountingExtractor {
                peak,
                current,
                sleep_ms: 0,
            }),
        ]);
        // Provider whose generic fetch ALSO fails, so the failing specialist
        // source genuinely falls through to the note (not the generic rescue).
        let svc = service_with_sources(
            enrich_config(),
            router,
            Some(Arc::new(SearchOkFetchErrProvider)),
        );
        let out = svc
            .web_search(base_input())
            .await
            .expect("web_search returns Ok despite one failure");

        let failed = out
            .sources
            .iter()
            .find(|s| s.url.contains("openai.com"))
            .expect("grok source present");
        let passed = out
            .sources
            .iter()
            .find(|s| s.url.contains("example.com"))
            .expect("supplemental source present");

        assert!(
            failed
                .content
                .as_deref()
                .unwrap_or("")
                .starts_with("_Failed to retrieve:"),
            "failing source must carry a failure note, got: {:?}",
            failed.content
        );
        let pc = passed.content.as_deref().unwrap_or("");
        assert!(
            !pc.is_empty() && !pc.starts_with("_Failed to retrieve:"),
            "passing source must carry real content, got: {pc:?}"
        );
    }

    #[tokio::test]
    async fn enrich_specialist_failure_rescued_by_generic_fetch() {
        // A matched specialist whose API errors must fall back to the configured
        // generic fetch (mirroring web_fetch), not store a failure note, when a
        // source provider can still fetch the URL.
        let router = SourceRouter::with_extractors(vec![Box::new(MarkerErrExtractor {
            fail_url_marker: "openai.com".to_string(),
        })]);
        let svc = service_with(enrich_config(), router); // FakeSourceProvider.fetch succeeds
        let out = svc.web_search(base_input()).await.expect("web_search");

        let failed = out
            .sources
            .iter()
            .find(|s| s.url.contains("openai.com"))
            .expect("grok source present");
        let content = failed.content.as_deref().unwrap_or("");
        assert!(
            content.starts_with("Fetched content from"),
            "specialist failure must be rescued by generic fetch, got: {content:?}"
        );
        assert!(
            !content.starts_with("_Failed to retrieve:"),
            "must not store a failure note when generic fetch succeeds: {content:?}"
        );
    }

    #[tokio::test]
    async fn generic_fetch_missing_config_only_when_no_provider_configured() {
        let err = generic_source_fetch(&[], "https://example.com")
            .await
            .expect_err("no providers must error");
        assert!(
            matches!(
                err,
                GrokSearchError::MissingConfig(
                    "TAVILY_API_KEY, EXA_API_KEY, TINYFISH_API_KEY or FIRECRAWL_API_KEY"
                )
            ),
            "expected MissingConfig, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn generic_fetch_primary_error_surfaces_without_fallback() {
        // Regression: a configured primary whose fetch failed used to fall
        // through to MissingConfig("TAVILY_API_KEY or FIRECRAWL_API_KEY") even
        // though TAVILY_API_KEY was set, sending users to debug config instead
        // of the actual provider failure.
        let chain = vec![SourceEntry {
            spec: &TAVILY_SPEC,
            provider: Arc::new(SearchOkFetchErrProvider),
        }];
        let err = generic_source_fetch(&chain, "https://example.com")
            .await
            .expect_err("primary failure must error");
        match err {
            GrokSearchError::Provider(msg) => assert_eq!(msg, "generic fetch unavailable"),
            other => panic!("primary error must pass through unchanged, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn generic_fetch_primary_empty_content_reports_empty_without_fallback() {
        let url = "https://npmjs.com/package/grok-search-rs";
        let chain = vec![SourceEntry {
            spec: &TAVILY_SPEC,
            provider: Arc::new(EmptyFetchProvider),
        }];
        let err = generic_source_fetch(&chain, url)
            .await
            .expect_err("empty content must error");
        match err {
            GrokSearchError::Provider(msg) => assert!(
                msg.contains("empty content") && msg.contains(url),
                "message must name the empty result and url, got: {msg}"
            ),
            other => panic!("expected Provider empty-content error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn generic_fetch_primary_failure_still_rescued_by_fallback() {
        let chain = vec![
            SourceEntry {
                spec: &TAVILY_SPEC,
                provider: Arc::new(SearchOkFetchErrProvider),
            },
            SourceEntry {
                spec: &FIRECRAWL_SPEC,
                provider: Arc::new(FakeSourceProvider),
            },
        ];
        let page = generic_source_fetch(&chain, "https://example.com")
            .await
            .expect("fallback must rescue primary failure");
        assert!(
            page.content.starts_with("Fetched content from"),
            "fallback content expected, got: {:?}",
            page.content
        );
    }

    #[tokio::test]
    async fn enrich_timeout_yields_note_not_error() {
        let router = SourceRouter::with_extractors(vec![Box::new(HangingExtractor)]);
        let mut config = enrich_config();
        config.timeout = Duration::from_millis(50); // deadline fires fast
        let svc = service_with(config, router);

        let out = svc
            .web_search(base_input())
            .await
            .expect("web_search returns Ok on timeout");
        for s in &out.sources {
            assert!(
                s.content.as_deref().unwrap_or("").contains("timeout"),
                "expected timeout note, got: {:?}",
                s.content
            );
        }
    }

    #[tokio::test]
    async fn include_content_false_omits_content_field() {
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let router = SourceRouter::with_extractors(vec![Box::new(CountingExtractor {
            peak,
            current,
            sleep_ms: 0,
        })]);
        let svc = service_with(enrich_config(), router);

        let mut input = base_input();
        input.include_content = Some(false);
        let out = svc.web_search(input).await.expect("web_search");

        for s in &out.sources {
            assert!(s.content.is_none());
            let value = serde_json::to_value(s).unwrap();
            assert!(
                value.get("content").is_none(),
                "JSON must omit the content key, not emit null"
            );
        }
    }

    #[tokio::test]
    async fn extra_sources_zero_suppresses_inline() {
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let router = SourceRouter::with_extractors(vec![Box::new(CountingExtractor {
            peak,
            current,
            sleep_ms: 0,
        })]);
        let svc = service_with(enrich_config(), router);

        let mut input = base_input();
        input.extra_sources = Some(0); // effective_extra_sources == 0 → dual gate suppresses enrich
        let out = svc.web_search(input).await.expect("web_search");

        for s in &out.sources {
            assert!(
                s.content.is_none(),
                "extra_sources=0 must keep the legacy no-content shape"
            );
        }
    }

    #[tokio::test]
    async fn get_sources_inherits_enriched_content() {
        let peak = Arc::new(AtomicUsize::new(0));
        let current = Arc::new(AtomicUsize::new(0));
        let router = SourceRouter::with_extractors(vec![Box::new(CountingExtractor {
            peak,
            current,
            sleep_ms: 0,
        })]);
        let svc = service_with(enrich_config(), router);

        let out = svc.web_search(base_input()).await.expect("web_search");
        let again = svc
            .get_sources(&out.session_id, 0, None)
            .await
            .expect("get_sources");

        assert_eq!(out.sources.len(), again.sources.len());
        for (a, b) in out.sources.iter().zip(again.sources.iter()) {
            assert_eq!(a.url, b.url);
            assert_eq!(
                a.content, b.content,
                "get_sources must reuse the cached enriched content"
            );
        }
    }

    /// Always-matching extractor returning markdown with a leading heading.
    struct HeadingExtractor {
        markdown: &'static str,
    }
    #[async_trait]
    impl SourceExtractor for HeadingExtractor {
        fn matches(&self, _url: &Url) -> bool {
            true
        }
        fn kind(&self) -> SourceType {
            SourceType::Wikipedia
        }
        async fn fetch_render(
            &self,
            _c: &reqwest::Client,
            _u: &Url,
            _caps: &SourceCaps,
        ) -> Result<String> {
            Ok(self.markdown.to_string())
        }
    }

    /// Generic provider whose fetch returns a page with full metadata.
    struct MetaFetchProvider;
    #[async_trait]
    impl SourceProvider for MetaFetchProvider {
        async fn search_sources(
            &self,
            _query: &str,
            _max_results: usize,
            _filters: &SearchFilters,
        ) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
        async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
            Ok(FetchedPage {
                content: "Page body.".to_string(),
                title: Some("Provider Title".to_string()),
                published_date: Some("2026-06-19T06:15:24-08:00".to_string()),
            })
        }
        async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
            Ok(Vec::new())
        }
    }

    /// Drive enrich_sources directly with a permissive deadline/caps so the
    /// backfill rules can be asserted without a full web_search round-trip.
    async fn run_enrich(
        sources: Vec<Source>,
        router: SourceRouter,
        primary: Option<Arc<dyn SourceProvider>>,
        max_sources: usize,
    ) -> Vec<Source> {
        let chain = primary
            .map(|provider| {
                vec![SourceEntry {
                    spec: &TAVILY_SPEC,
                    provider,
                }]
            })
            .unwrap_or_default();
        enrich_sources(
            sources,
            tokio::time::Instant::now() + Duration::from_secs(30),
            &crate::providers::http::build_client(Duration::from_secs(5)),
            &Arc::new(router),
            SourceCaps {
                max_answers: 3,
                max_comments: 3,
            },
            4,
            15_000,
            max_sources,
            chain,
        )
        .await
    }

    fn bare_source(url: &str) -> Source {
        Source::new(url, "grok_responses")
    }

    #[tokio::test]
    async fn backfill_title_from_specialist_heading() {
        let router = SourceRouter::with_extractors(vec![Box::new(HeadingExtractor {
            markdown: "# Real Title\n\nBody text.",
        })]);
        let out = run_enrich(vec![bare_source("https://example.com/a")], router, None, 5).await;

        assert_eq!(out[0].title.as_deref(), Some("Real Title"));
        assert_eq!(out[0].published_date, None, "specialists carry no date");
    }

    #[tokio::test]
    async fn backfill_metadata_from_generic_provider() {
        // Empty router → no specialist matches → generic provider fetch path.
        let out = run_enrich(
            vec![bare_source("https://example.com/a")],
            SourceRouter::with_extractors(Vec::new()),
            Some(Arc::new(MetaFetchProvider)),
            5,
        )
        .await;

        assert_eq!(out[0].title.as_deref(), Some("Provider Title"));
        assert_eq!(
            out[0].published_date.as_deref(),
            Some("2026-06-19T06:15:24-08:00")
        );
    }

    #[tokio::test]
    async fn backfill_never_overwrites_upstream_metadata() {
        let source = bare_source("https://example.com/a")
            .with_title("Upstream Title")
            .with_published_date("2020-01-01");
        let out = run_enrich(
            vec![source],
            SourceRouter::with_extractors(Vec::new()),
            Some(Arc::new(MetaFetchProvider)),
            5,
        )
        .await;

        assert_eq!(out[0].title.as_deref(), Some("Upstream Title"));
        assert_eq!(out[0].published_date.as_deref(), Some("2020-01-01"));
    }

    #[tokio::test]
    async fn backfill_skipped_on_failed_fetch() {
        // No specialist + failing generic fetch → failure note, no metadata.
        let out = run_enrich(
            vec![bare_source("https://example.com/a")],
            SourceRouter::with_extractors(Vec::new()),
            Some(Arc::new(SearchOkFetchErrProvider)),
            5,
        )
        .await;

        assert!(out[0]
            .content
            .as_deref()
            .unwrap_or("")
            .starts_with("_Failed to retrieve:"));
        assert_eq!(out[0].title, None, "failure notes must not become titles");
        assert_eq!(out[0].published_date, None);
    }

    #[tokio::test]
    async fn backfill_rejects_junk_heading() {
        // First heading is a citation-index artifact — no guessing from later
        // section headings either.
        let router = SourceRouter::with_extractors(vec![Box::new(HeadingExtractor {
            markdown: "# 1\n\n## Real Section\n\nBody.",
        })]);
        let out = run_enrich(vec![bare_source("https://example.com/a")], router, None, 5).await;

        assert_eq!(out[0].title, None);
    }

    #[tokio::test]
    async fn backfill_leaves_tail_beyond_window_untouched() {
        let router = SourceRouter::with_extractors(vec![Box::new(HeadingExtractor {
            markdown: "# Real Title\n\nBody.",
        })]);
        let sources = vec![
            bare_source("https://example.com/a"),
            bare_source("https://example.com/b"),
        ];
        let out = run_enrich(sources, router, None, 1).await;

        assert_eq!(out[0].title.as_deref(), Some("Real Title"));
        assert_eq!(out[1].title, None, "un-enriched tail keeps honest nulls");
        assert_eq!(out[1].content, None);
    }

    #[test]
    fn first_markdown_heading_extracts_and_rejects() {
        assert_eq!(
            first_markdown_heading("# Title Line\nbody"),
            Some("Title Line".to_string())
        );
        assert_eq!(
            first_markdown_heading("prose first\n\n## Section Two\n"),
            Some("Section Two".to_string()),
            "first heading may appear after prose"
        );
        assert_eq!(first_markdown_heading("no headings at all"), None);
        assert_eq!(
            first_markdown_heading("#hashtag is prose"),
            None,
            "ATX heading requires whitespace after the marker"
        );
        assert_eq!(first_markdown_heading("####### seven hashes"), None);
        assert_eq!(
            first_markdown_heading("# 1\n\n# Real"),
            None,
            "junk first heading must not fall through to later ones"
        );
        let oversized = format!("# {}", "x".repeat(MAX_HEADING_TITLE_CHARS + 1));
        assert_eq!(first_markdown_heading(&oversized), None);
    }
}

#[cfg(test)]
mod request_scope_tests {
    use super::*;

    /// A long-lived template service, as the HTTP transport would hold one.
    fn template() -> SearchService {
        SearchService::new(Config::from_env_map([(
            "GROK_SEARCH_API_KEY",
            "xai-template",
        )]))
        .expect("template service builds")
    }

    #[test]
    fn with_config_rejects_oauth() {
        let svc = template();
        let cfg = Config::from_env_map([("GROK_SEARCH_AUTH_MODE", "oauth")]);
        assert!(
            svc.with_config(cfg).is_err(),
            "OAuth must be rejected on the per-request (HTTP) path"
        );
    }

    #[test]
    fn with_config_fails_closed_without_grok_key() {
        let svc = template();
        // No grok key and no OpenAI-compatible gateway -> Responses transport ->
        // construction fails rather than silently reusing the template's key.
        let cfg = Config::from_env_map(Vec::<(String, String)>::new());
        assert!(
            svc.with_config(cfg).is_err(),
            "missing required key must fail closed, never fall back to the server key"
        );
    }

    #[test]
    fn with_config_accepts_per_request_key() {
        let svc = template();
        let cfg = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "xai-caller"),
            ("TAVILY_API_KEY", "tvly-caller"),
        ]);
        assert!(
            svc.with_config(cfg).is_ok(),
            "a caller-supplied key must build a request-scoped service"
        );
    }

    #[tokio::test]
    async fn cache_shared_within_tenant_isolated_across_tenants() {
        let svc = template(); // key "xai-template"
        let session = "abcdef012345";

        // Same-key request-scoped service shares cached sessions, so
        // get_sources continuation survives across requests.
        let same = svc
            .with_config(Config::from_env_map([(
                "GROK_SEARCH_API_KEY",
                "xai-template",
            )]))
            .expect("same-tenant scoped service");
        // Seed under the tenant-namespaced key, as web_search would.
        same.cache.lock().await.set(
            same.tenant_cache_key(session),
            Arc::new(Vec::<Source>::new()),
        );
        assert!(
            same.get_sources(session, 0, None).await.is_ok(),
            "same tenant must read its own cached session"
        );

        // A different caller key must NOT read that session.
        let other = svc
            .with_config(Config::from_env_map([("GROK_SEARCH_API_KEY", "xai-other")]))
            .expect("other-tenant scoped service");
        assert!(
            other.get_sources(session, 0, None).await.is_err(),
            "a different tenant must not read another tenant's cached session"
        );
    }

    #[test]
    fn tenant_tag_namespaces_by_gateway() {
        // Same opaque key on two different gateways must NOT share a cache
        // namespace: with arbitrary public gateways, independent gateways can
        // issue/accept identical key strings for different callers.
        let on_xai = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "same-key"),
            ("GROK_SEARCH_URL", "https://api.x.ai"),
        ]);
        let on_other = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "same-key"),
            ("GROK_SEARCH_URL", "https://gateway.example"),
        ]);
        assert_ne!(
            tenant_tag(&on_xai),
            tenant_tag(&on_other),
            "gateway must be part of the tenant namespace"
        );
        // Same key + same gateway stays stable (continuation still works).
        let again = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "same-key"),
            ("GROK_SEARCH_URL", "https://api.x.ai"),
        ]);
        assert_eq!(tenant_tag(&on_xai), tenant_tag(&again));
    }
}
