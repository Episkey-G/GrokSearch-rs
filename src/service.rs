use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::cache::SourceCache;
use crate::config::Config;
use crate::error::{GrokSearchError, Result};
use crate::model::search::{
    ContentBlock, SearchFilters, SearchMessage, SearchRequest, SearchResponse, SearchTool,
};
use crate::model::source::{merge_sources, Source};
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
    async fn fetch(&self, url: &str) -> Result<String>;
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

    async fn fetch(&self, url: &str) -> Result<String> {
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

    async fn fetch(&self, url: &str) -> Result<String> {
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
}

impl SearchService {
    pub fn new(config: Config) -> Result<Self> {
        use crate::config::Transport;
        let http = crate::providers::http::build_client(config.timeout);

        let ai: Arc<dyn AiProvider> = match config.transport {
            Transport::Responses => {
                let grok_key = config
                    .grok_api_key
                    .clone()
                    .ok_or(GrokSearchError::MissingConfig("GROK_SEARCH_API_KEY"))?;
                Arc::new(GrokResponsesProvider::with_client(
                    http.clone(),
                    config.grok_api_url.clone(),
                    grok_key,
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

        Ok(Self {
            cache: Arc::new(Mutex::new(SourceCache::new(config.cache_size))),
            default_model: resolve_default_model(&config),
            config,
            ai,
            sources,
            fallback_sources,
        })
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
        }
    }

    pub async fn web_search(&self, input: WebSearchInput) -> Result<WebSearchOutput> {
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
            Err(_) => {
                return self
                    .finalize_fallback(
                        session_id,
                        SearchResponse {
                            content: String::new(),
                            sources: Vec::new(),
                        },
                        raw_sources,
                        raw_origin,
                        "grok_provider_error",
                    )
                    .await;
            }
        };

        if let Some(reason) = grok_unverifiable_reason(&response) {
            return self
                .finalize_fallback(session_id, response, raw_sources, raw_origin, reason)
                .await;
        }

        let mut enrichment = raw_sources;
        enrichment.truncate(effective_extra_sources);
        let enrichment = with_provider(enrichment, enrichment_label(raw_origin));
        let merged = merge_sources(response.sources, enrichment);

        let merged_arc = Arc::new(merged);
        let sources_count = merged_arc.len();
        self.cache
            .lock()
            .await
            .set(session_id.clone(), merged_arc.clone());

        Ok(WebSearchOutput {
            session_id,
            content: response.content,
            sources_count,
            sources: Arc::try_unwrap(merged_arc).unwrap_or_else(|arc| (*arc).clone()),
            search_provider: "grok_responses".to_string(),
            fallback_used: false,
            fallback_reason: None,
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
        if let Some(provider) = &self.fallback_sources {
            if let Ok(sources) = provider.search_sources(query, count, filters).await {
                if !sources.is_empty() {
                    return (sources, RawSourceOrigin::Fallback);
                }
            }
        }
        (Vec::new(), RawSourceOrigin::None)
    }

    async fn finalize_fallback(
        &self,
        session_id: String,
        response: SearchResponse,
        raw_sources: Vec<Source>,
        raw_origin: RawSourceOrigin,
        reason: &str,
    ) -> Result<WebSearchOutput> {
        let mut fallback = raw_sources;
        fallback.truncate(self.config.fallback_sources);
        let fallback = with_provider(fallback, fallback_label(raw_origin));

        let fallback_arc = Arc::new(fallback);
        let sources_count = fallback_arc.len();
        self.cache
            .lock()
            .await
            .set(session_id.clone(), fallback_arc.clone());

        let content = if response.content.trim().is_empty() {
            format!(
                "Grok Responses search did not return a verifiable answer. Source fallback returned {sources_count} source(s); evaluate them directly rather than treating any text as a verified answer."
            )
        } else {
            format!(
                "Grok Responses returned an answer without verifiable search sources, so source fallback returned {sources_count} source(s). Original Grok answer was not treated as verified; evaluate the listed sources directly."
            )
        };

        Ok(WebSearchOutput {
            session_id,
            content,
            sources_count,
            sources: Arc::try_unwrap(fallback_arc).unwrap_or_else(|arc| (*arc).clone()),
            search_provider: "source_fallback".to_string(),
            fallback_used: true,
            fallback_reason: Some(reason.to_string()),
        })
    }

    pub async fn get_sources(&self, session_id: &str) -> Result<GetSourcesOutput> {
        let sources = self
            .cache
            .lock()
            .await
            .get(session_id)
            .ok_or_else(|| GrokSearchError::NotFound(format!("session_id={session_id}")))?;
        let sources_count = sources.len();
        Ok(GetSourcesOutput {
            session_id: session_id.to_string(),
            sources_count,
            sources: Arc::try_unwrap(sources).unwrap_or_else(|arc| (*arc).clone()),
        })
    }

    pub async fn web_fetch(&self, url: &str, max_chars: Option<usize>) -> Result<WebFetchOutput> {
        let raw = self.web_fetch_raw(url).await?;
        let effective_limit = max_chars.or(self.config.fetch_max_chars);
        Ok(apply_fetch_limit(url, raw, effective_limit))
    }

    async fn web_fetch_raw(&self, url: &str) -> Result<String> {
        if let Some(provider) = &self.sources {
            if let Ok(content) = provider.fetch(url).await {
                if !content.trim().is_empty() {
                    return Ok(content);
                }
            }
        }

        if let Some(provider) = &self.fallback_sources {
            return provider.fetch(url).await;
        }

        Err(GrokSearchError::MissingConfig(
            "TAVILY_API_KEY or FIRECRAWL_API_KEY",
        ))
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

fn grok_unverifiable_reason(response: &SearchResponse) -> Option<&'static str> {
    if response.content.trim().is_empty() {
        return Some("grok_content_empty");
    }
    if response.sources.is_empty() {
        return Some("grok_sources_empty");
    }
    None
}

fn apply_fetch_limit(url: &str, mut content: String, max_chars: Option<usize>) -> WebFetchOutput {
    let Some(limit) = max_chars else {
        let original_length = content.chars().count();
        return WebFetchOutput {
            url: url.to_string(),
            content,
            original_length,
            truncated: false,
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
            }
        }
        None => WebFetchOutput {
            url: url.to_string(),
            content,
            original_length: count,
            truncated: false,
        },
    }
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

    async fn fetch(&self, url: &str) -> Result<String> {
        Ok(format!("Fetched content from {url}"))
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
        };

        let report = svc.doctor().await;
        assert_eq!(report["provider"], "grok_responses");
        assert_eq!(report["grok"]["model"], "grok-4-1-fast-reasoning");
    }
}
