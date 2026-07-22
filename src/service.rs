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
use crate::providers::firecrawl::FirecrawlProvider;
use crate::providers::grok::GrokResponsesProvider;
use crate::providers::tavily::TavilyProvider;

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
    sources: Option<Arc<dyn SourceProvider>>,
    fallback_sources: Option<Arc<dyn SourceProvider>>,
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
    sources: Option<Arc<dyn SourceProvider>>,
    fallback_sources: Option<Arc<dyn SourceProvider>>,
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

    let sources = if config.tavily_enabled {
        config.tavily_api_key.clone().map(|key| {
            Arc::new(TavilyProvider::with_client(
                http.clone(),
                config.tavily_api_url.clone(),
                key,
            )) as Arc<dyn SourceProvider>
        })
    } else {
        None
    };

    let fallback_sources = if config.firecrawl_enabled {
        config.firecrawl_api_key.clone().map(|key| {
            Arc::new(FirecrawlProvider::with_client(
                http.clone(),
                config.firecrawl_api_url.clone(),
                key,
            )) as Arc<dyn SourceProvider>
        })
    } else {
        None
    };

    let source_router = Arc::new(crate::sources::SourceRouter::from_config(config));

    Ok(ProviderSet {
        ai,
        default_model: resolve_default_model(config),
        sources,
        fallback_sources,
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
            sources: providers.sources,
            fallback_sources: providers.fallback_sources,
            http_client: http,
            source_router: providers.source_router,
        }
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
        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            sources: Some(Arc::new(FakeSourceProvider)),
            fallback_sources: None,
            http_client: crate::providers::http::build_client(std::time::Duration::from_secs(30)),
            source_router: Arc::new(crate::sources::SourceRouter::default()),
        }
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

        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            default_model: resolve_default_model(&config),
            config,
            ai: ai.unwrap_or_else(|| Arc::new(FakeAiProvider)),
            sources: Some(primary),
            fallback_sources: fallback,
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
        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            sources: Some(primary),
            fallback_sources: fallback,
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
            self.fetch_raw_extra_sources(&input.query, speculative_count, &filters);
        let (grok_result, (raw_sources, raw_origin)) =
            tokio::join!(grok_future, speculative_future);

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
                        raw_sources,
                        raw_origin,
                        grok_error_reason(&err),
                        include_content,
                    )
                    .await;
            }
        };

        if let Some(reason) = grok_unverifiable_reason(&response) {
            return self
                .finalize_fallback(
                    deadline,
                    session_id,
                    response,
                    raw_sources,
                    raw_origin,
                    reason,
                    include_content,
                )
                .await;
        }

        let mut enrichment = raw_sources;
        enrichment.truncate(effective_extra_sources);
        let enrichment = with_provider(enrichment, enrichment_label(raw_origin));
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
                self.sources.clone(),
                self.fallback_sources.clone(),
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

    /// Fetch sources from the primary source provider (or fall through to
    /// firecrawl) without applying a path-specific provider label. The
    /// returned Vec carries each provider's native label ("tavily"/"firecrawl");
    /// the caller re-labels via `with_provider` once the path (enrichment vs
    /// fallback) is known.
    async fn fetch_raw_extra_sources(
        &self,
        query: &str,
        count: usize,
        filters: &SearchFilters,
    ) -> (Vec<Source>, RawSourceOrigin) {
        if count == 0 {
            return (Vec::new(), RawSourceOrigin::None);
        }
        if let Some(provider) = &self.sources {
            if let Ok(sources) = provider.search_sources(query, count, filters).await {
                if !sources.is_empty() {
                    return (sources, RawSourceOrigin::Primary);
                }
            }
        }
        // The fallback source provider (Firecrawl) ignores `SearchFilters`
        // outright — it has no structured recency/domain support — so a
        // filter-constrained request must never fall through: swapping
        // constrained Tavily results for unconstrained Firecrawl ones would
        // silently violate the include/exclude/recency contract. Constrained
        // requests are Tavily-or-nothing.
        if filters.is_empty() {
            if let Some(provider) = &self.fallback_sources {
                if let Ok(sources) = provider.search_sources(query, count, filters).await {
                    if !sources.is_empty() {
                        return (sources, RawSourceOrigin::Fallback);
                    }
                }
            }
        }
        (Vec::new(), RawSourceOrigin::None)
    }

    #[allow(clippy::too_many_arguments)]
    async fn finalize_fallback(
        &self,
        deadline: tokio::time::Instant,
        session_id: String,
        response: SearchResponse,
        raw_sources: Vec<Source>,
        raw_origin: RawSourceOrigin,
        reason: &str,
        include_content: bool,
    ) -> Result<WebSearchOutput> {
        let mut fallback = raw_sources;
        fallback.truncate(self.config.fallback_sources);
        let fallback = with_provider(fallback, fallback_label(raw_origin));

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
                self.sources.clone(),
                self.fallback_sources.clone(),
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

        let (content, source_type, fallback_reason) = match url::Url::parse(url) {
            Ok(parsed) => {
                match crate::sources::resolve_content(
                    &self.http_client,
                    &parsed,
                    self.source_router.as_ref(),
                    &crate::sources::SourceCaps {
                        max_answers: self.config.source_max_answers,
                        max_comments: self.config.source_max_comments,
                    },
                )
                .await
                {
                    // Specialist succeeded — keep its content and source type.
                    Ok((content, kind)) => (content, kind, None),
                    // No specialist matched: go generic silently (D-01).
                    Err(reason) if reason == crate::sources::NO_SPECIALIST_MATCH => {
                        let generic = self.web_fetch_raw(url).await?;
                        (generic, crate::sources::SourceType::Generic, None)
                    }
                    // Specialist matched but failed/empty: surface the reason (D-01).
                    Err(reason) => {
                        let generic = self.web_fetch_raw(url).await?;
                        (generic, crate::sources::SourceType::Generic, Some(reason))
                    }
                }
            }
            // Malformed URL is not a specialist failure — go generic, no reason.
            Err(_) => {
                let generic = self.web_fetch_raw(url).await?;
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

    async fn web_fetch_raw(&self, url: &str) -> Result<String> {
        generic_source_fetch(&self.sources, &self.fallback_sources, url)
            .await
            .map(|page| page.content)
    }

    pub async fn web_map(&self, url: &str, max_results: usize) -> Result<Vec<Source>> {
        self.sources
            .as_ref()
            .ok_or(GrokSearchError::MissingConfig("TAVILY_API_KEY"))?
            .map(url, max_results)
            .await
    }

    /// Runtime diagnostics with live connectivity probes against each configured backend.
    /// Returns provider availability flags, masked config, and per-provider reachability.
    pub async fn doctor(&self) -> serde_json::Value {
        use crate::config::Transport;
        let grok_probe = self.probe_grok().await;
        let tavily_probe = match &self.sources {
            Some(provider) => probe_source(provider.as_ref(), "https://example.com").await,
            None => Probe::skipped("TAVILY_API_KEY not configured"),
        };
        let firecrawl_probe = match &self.fallback_sources {
            Some(provider) => probe_source(provider.as_ref(), "https://example.com").await,
            None => Probe::skipped("FIRECRAWL_API_KEY not configured"),
        };

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
            "firecrawl": {
                "api_url": self.config.firecrawl_api_url,
                "enabled": self.config.firecrawl_enabled,
                "reachable": firecrawl_probe.ok,
                "detail": firecrawl_probe.detail,
            },
            "default_extra_sources": self.config.default_extra_sources,
            "fallback_sources": self.config.fallback_sources,
            "cache_size": self.config.cache_size,
            "timeout_seconds": self.config.timeout.as_secs(),
            "github_token": self.config.github_token_status(),
            "redacted": self.config.redacted_diagnostics()
        })
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

#[derive(Debug, Clone, Copy)]
enum RawSourceOrigin {
    None,
    Primary,
    Fallback,
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

fn enrichment_label(origin: RawSourceOrigin) -> &'static str {
    match origin {
        RawSourceOrigin::Primary => "tavily_enrichment",
        RawSourceOrigin::Fallback => "firecrawl_enrichment",
        RawSourceOrigin::None => "tavily_enrichment",
    }
}

fn fallback_label(origin: RawSourceOrigin) -> &'static str {
    match origin {
        RawSourceOrigin::Primary => "tavily_fallback",
        RawSourceOrigin::Fallback => "firecrawl_enrichment",
        RawSourceOrigin::None => "tavily_fallback",
    }
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

/// Generic (non-specialist) content fetch via the configured source providers:
/// primary (Tavily) first, then fallback (Firecrawl). Shared by `web_fetch` and
/// inline enrichment so both agree on how an ordinary URL is retrieved once no
/// specialist extractor matches. Returns `MissingConfig` only when neither
/// provider is configured; a configured primary that fails or yields empty
/// content with no fallback surfaces its real error instead, so users are not
/// sent to debug config that is actually set.
async fn generic_source_fetch(
    primary: &Option<Arc<dyn SourceProvider>>,
    fallback: &Option<Arc<dyn SourceProvider>>,
    url: &str,
) -> Result<FetchedPage> {
    let primary_err = match primary {
        Some(provider) => match provider.fetch(url).await {
            Ok(page) if !page.content.trim().is_empty() => return Ok(page),
            Ok(_) => GrokSearchError::Provider(format!("Tavily returned empty content for {url}")),
            Err(err) => err,
        },
        None => GrokSearchError::MissingConfig("TAVILY_API_KEY or FIRECRAWL_API_KEY"),
    };
    match fallback {
        Some(provider) => provider.fetch(url).await,
        None => Err(primary_err),
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
    primary: Option<Arc<dyn SourceProvider>>,
    fallback: Option<Arc<dyn SourceProvider>>,
) -> Vec<Source> {
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut set: tokio::task::JoinSet<(usize, EnrichedFetch)> = tokio::task::JoinSet::new();

    for (idx, source) in sources.iter().enumerate().take(max_sources) {
        let permit = Arc::clone(&sem);
        let url_str = source.url.clone();
        let client = client.clone();
        let router = Arc::clone(router);
        let caps = caps.clone();
        let primary = primary.clone();
        let fallback = fallback.clone();

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
                        // surfaced only if the generic fetch also fails.
                        Ok(Err(reason)) => {
                            let generic = generic_source_fetch(&primary, &fallback, &url_str);
                            match tokio::time::timeout_at(deadline, generic).await {
                                Ok(Ok(page)) => EnrichedFetch::from_page(page, max_chars),
                                Ok(Err(_)) => EnrichedFetch::note(format!(
                                    "_Failed to retrieve: {reason}_\n\nSource: {url_str}"
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
            sources: None,
            fallback_sources: None,
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
            sources: None,
            fallback_sources: None,
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
            sources: None,
            fallback_sources: None,
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
            sources: None,
            fallback_sources: None,
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
        SearchService {
            default_model: resolve_default_model(&config),
            config,
            ai: Arc::new(FakeAiProvider),
            sources,
            fallback_sources: None,
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
        let err = generic_source_fetch(&None, &None, "https://example.com")
            .await
            .expect_err("no providers must error");
        assert!(
            matches!(
                err,
                GrokSearchError::MissingConfig("TAVILY_API_KEY or FIRECRAWL_API_KEY")
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
        let primary: Option<Arc<dyn SourceProvider>> = Some(Arc::new(SearchOkFetchErrProvider));
        let err = generic_source_fetch(&primary, &None, "https://example.com")
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
        let primary: Option<Arc<dyn SourceProvider>> = Some(Arc::new(EmptyFetchProvider));
        let err = generic_source_fetch(&primary, &None, url)
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
        let primary: Option<Arc<dyn SourceProvider>> = Some(Arc::new(SearchOkFetchErrProvider));
        let fallback: Option<Arc<dyn SourceProvider>> = Some(Arc::new(FakeSourceProvider));
        let page = generic_source_fetch(&primary, &fallback, "https://example.com")
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
            primary,
            None,
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
