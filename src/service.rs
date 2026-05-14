use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use uuid::Uuid;

use crate::cache::SourceCache;
use crate::config::Config;
use crate::error::{GrokSearchError, Result};
use crate::model::search::{
    ContentBlock, SearchMessage, SearchRequest, SearchResponse, SearchTool,
};
use crate::model::source::{merge_sources, Source};
use crate::model::tool::{GetSourcesOutput, WebSearchInput, WebSearchOutput};
use crate::planning::{PlanResult, PlanningEngine};
use crate::providers::firecrawl::FirecrawlProvider;
use crate::providers::grok::GrokResponsesProvider;
use crate::providers::tavily::TavilyProvider;

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn search(&self, request: &SearchRequest) -> Result<SearchResponse>;
}

#[async_trait]
pub trait SourceProvider: Send + Sync {
    async fn search_sources(&self, query: &str, max_results: usize) -> Result<Vec<Source>>;
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
impl SourceProvider for TavilyProvider {
    async fn search_sources(&self, query: &str, max_results: usize) -> Result<Vec<Source>> {
        self.search(query, max_results).await
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
    async fn search_sources(&self, query: &str, max_results: usize) -> Result<Vec<Source>> {
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
    sources: Option<Arc<dyn SourceProvider>>,
    fallback_sources: Option<Arc<dyn SourceProvider>>,
    cache: Arc<Mutex<SourceCache>>,
    planning: Arc<Mutex<PlanningEngine>>,
}

impl SearchService {
    pub fn new(config: Config) -> Result<Self> {
        let grok_key = config
            .grok_api_key
            .clone()
            .ok_or(GrokSearchError::MissingConfig("GROK_SEARCH_API_KEY"))?;
        let ai: Arc<dyn AiProvider> = Arc::new(GrokResponsesProvider::new(
            config.grok_api_url.clone(),
            grok_key,
            config.web_search_enabled,
            config.x_search_enabled,
            config.timeout,
        ));

        let sources = if config.tavily_enabled {
            config.tavily_api_key.clone().map(|key| {
                Arc::new(TavilyProvider::new(
                    config.tavily_api_url.clone(),
                    key,
                    config.timeout,
                )) as Arc<dyn SourceProvider>
            })
        } else {
            None
        };

        let fallback_sources = if config.firecrawl_enabled {
            config.firecrawl_api_key.clone().map(|key| {
                Arc::new(FirecrawlProvider::new(
                    config.firecrawl_api_url.clone(),
                    key,
                    config.timeout,
                )) as Arc<dyn SourceProvider>
            })
        } else {
            None
        };

        Ok(Self {
            cache: Arc::new(Mutex::new(SourceCache::new(config.cache_size))),
            planning: Arc::new(Mutex::new(PlanningEngine::default())),
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
            planning: Arc::new(Mutex::new(PlanningEngine::default())),
            config,
            ai: Arc::new(FakeAiProvider),
            sources: Some(Arc::new(FakeSourceProvider)),
            fallback_sources: None,
        }
    }

    pub fn fake_with_custom_sources_and_config<I, K, V>(
        sources: Arc<dyn SourceProvider>,
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
        vars.extend(
            overrides
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        let config = Config::from_env_map(vars);

        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            planning: Arc::new(Mutex::new(PlanningEngine::default())),
            config,
            ai: Arc::new(FakeAiProvider),
            sources: Some(sources),
            fallback_sources: None,
        }
    }

    pub fn fake_with_ai_and_sources<I, K, V>(
        ai: Arc<dyn AiProvider>,
        sources: Arc<dyn SourceProvider>,
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
        vars.extend(
            overrides
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        let config = Config::from_env_map(vars);

        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            planning: Arc::new(Mutex::new(PlanningEngine::default())),
            config,
            ai,
            sources: Some(sources),
            fallback_sources: None,
        }
    }

    pub fn fake_with_primary_and_fallback_sources<I, K, V>(
        primary: Arc<dyn SourceProvider>,
        fallback: Arc<dyn SourceProvider>,
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
            (
                "FIRECRAWL_API_KEY".to_string(),
                "fake-firecrawl".to_string(),
            ),
        ];
        vars.extend(
            overrides
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        let config = Config::from_env_map(vars);

        Self {
            cache: Arc::new(Mutex::new(SourceCache::new(256))),
            planning: Arc::new(Mutex::new(PlanningEngine::default())),
            config,
            ai: Arc::new(FakeAiProvider),
            sources: Some(primary),
            fallback_sources: Some(fallback),
        }
    }

    pub async fn web_search(&self, input: WebSearchInput) -> Result<WebSearchOutput> {
        let session_id = Uuid::new_v4().simple().to_string()[..12].to_string();
        let effective_extra_sources = input
            .extra_sources
            .unwrap_or(self.config.default_extra_sources);

        let request = self.build_search_request(&input, &[]);
        let response = match self.ai.search(&request).await {
            Ok(response) => response,
            Err(_) => {
                return self
                    .web_search_source_fallback(
                        session_id,
                        SearchResponse {
                            content: String::new(),
                            sources: Vec::new(),
                        },
                        &input,
                        "grok_provider_error",
                    )
                    .await;
            }
        };

        if let Some(reason) = grok_unverifiable_reason(&response) {
            return self
                .web_search_source_fallback(session_id, response, &input, reason)
                .await;
        }

        let extra_sources = self
            .search_extra_sources(&input.query, effective_extra_sources, "tavily_enrichment")
            .await;
        let merged = merge_sources(response.sources, extra_sources);
        let sources_count = merged.len();
        self.cache
            .lock()
            .expect("source cache poisoned")
            .set(session_id.clone(), merged);

        Ok(WebSearchOutput {
            session_id,
            content: response.content,
            sources_count,
            search_provider: "grok_responses".to_string(),
            fallback_used: false,
            fallback_reason: None,
        })
    }

    async fn search_extra_sources(
        &self,
        query: &str,
        count: usize,
        primary_provider: &str,
    ) -> Vec<Source> {
        if count == 0 {
            return Vec::new();
        }

        if let Some(provider) = &self.sources {
            let sources = provider
                .search_sources(query, count)
                .await
                .unwrap_or_default();
            if !sources.is_empty() {
                return with_provider(sources, primary_provider);
            }
        }

        if let Some(provider) = &self.fallback_sources {
            let sources = provider
                .search_sources(query, count)
                .await
                .unwrap_or_default();
            if !sources.is_empty() {
                return with_provider(sources, "firecrawl_enrichment");
            }
        }

        Vec::new()
    }

    async fn web_search_source_fallback(
        &self,
        session_id: String,
        response: SearchResponse,
        input: &WebSearchInput,
        reason: &str,
    ) -> Result<WebSearchOutput> {
        let fallback_sources = self
            .search_extra_sources(
                &input.query,
                self.config.fallback_sources,
                "tavily_fallback",
            )
            .await;
        let sources_count = fallback_sources.len();
        self.cache
            .lock()
            .expect("source cache poisoned")
            .set(session_id.clone(), fallback_sources);

        let content = if response.content.trim().is_empty() {
            format!(
                "Grok Responses search did not return a verifiable answer. Source fallback returned {sources_count} source(s)."
            )
        } else {
            format!(
                "Grok Responses returned an answer without verifiable search sources, so source fallback returned {sources_count} source(s). Original Grok answer was not treated as verified."
            )
        };

        Ok(WebSearchOutput {
            session_id,
            content,
            sources_count,
            search_provider: "source_fallback".to_string(),
            fallback_used: true,
            fallback_reason: Some(reason.to_string()),
        })
    }

    pub async fn get_sources(&self, session_id: &str) -> Result<GetSourcesOutput> {
        let sources = self
            .cache
            .lock()
            .expect("source cache poisoned")
            .get(session_id)
            .ok_or_else(|| GrokSearchError::Provider("session_id_not_found".to_string()))?;
        Ok(GetSourcesOutput {
            session_id: session_id.to_string(),
            sources_count: sources.len(),
            sources,
        })
    }

    pub async fn web_fetch(&self, url: &str) -> Result<String> {
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

    pub fn health(&self) -> String {
        self.config.redacted_diagnostics()
    }

    pub fn get_config_info(&self) -> serde_json::Value {
        serde_json::json!({
            "provider": "grok_responses",
            "grok_api_url": self.config.grok_api_url,
            "grok_model": self.config.grok_model,
            "web_search_enabled": self.config.web_search_enabled,
            "x_search_enabled": self.config.x_search_enabled,
            "tavily_api_url": self.config.tavily_api_url,
            "tavily_enabled": self.config.tavily_enabled,
            "firecrawl_api_url": self.config.firecrawl_api_url,
            "firecrawl_enabled": self.config.firecrawl_enabled,
            "default_extra_sources": self.config.default_extra_sources,
            "fallback_sources": self.config.fallback_sources,
            "cache_size": self.config.cache_size,
            "timeout_seconds": self.config.timeout.as_secs(),
            "redacted": self.config.redacted_diagnostics()
        })
    }

    pub fn switch_model(&self, model: &str) -> serde_json::Value {
        serde_json::json!({
            "status": "ok",
            "message": "model override is accepted per web_search call; persistent runtime mutation is not used in Rust service",
            "requested_model": model
        })
    }

    pub fn plan_intent(
        &self,
        session_id: &str,
        core_question: &str,
        query_type: &str,
        time_sensitivity: &str,
        confidence: f64,
    ) -> PlanResult {
        self.planning
            .lock()
            .expect("planning engine poisoned")
            .plan_intent(
                session_id,
                core_question,
                query_type,
                time_sensitivity,
                confidence,
            )
    }

    pub fn plan_search(
        &self,
        query: &str,
        complexity: &str,
        time_sensitivity: &str,
        confidence: f64,
    ) -> PlanResult {
        self.planning
            .lock()
            .expect("planning engine poisoned")
            .plan_search(query, complexity, time_sensitivity, confidence)
    }

    pub fn plan_complexity(
        &self,
        session_id: &str,
        level: u8,
        estimated_sub_queries: u32,
        estimated_tool_calls: u32,
        justification: &str,
        confidence: f64,
    ) -> PlanResult {
        self.planning
            .lock()
            .expect("planning engine poisoned")
            .plan_complexity(
                session_id,
                level,
                estimated_sub_queries,
                estimated_tool_calls,
                justification,
                confidence,
            )
    }

    pub fn plan_sub_query(
        &self,
        session_id: &str,
        id: &str,
        goal: &str,
        expected_output: &str,
        boundary: &str,
        confidence: f64,
    ) -> PlanResult {
        self.planning
            .lock()
            .expect("planning engine poisoned")
            .plan_sub_query(session_id, id, goal, expected_output, boundary, confidence)
    }

    pub fn plan_search_term(
        &self,
        session_id: &str,
        term: &str,
        purpose: &str,
        round: u32,
        approach: &str,
        confidence: f64,
    ) -> PlanResult {
        self.planning
            .lock()
            .expect("planning engine poisoned")
            .plan_search_term(session_id, term, purpose, round, approach, confidence)
    }

    pub fn plan_tool_mapping(
        &self,
        session_id: &str,
        sub_query_id: &str,
        tool: &str,
        reason: &str,
        confidence: f64,
    ) -> PlanResult {
        self.planning
            .lock()
            .expect("planning engine poisoned")
            .plan_tool_mapping(session_id, sub_query_id, tool, reason, confidence)
    }

    pub fn plan_execution(
        &self,
        session_id: &str,
        parallel: Vec<Vec<String>>,
        sequential: Vec<String>,
        estimated_rounds: u32,
        confidence: f64,
    ) -> PlanResult {
        self.planning
            .lock()
            .expect("planning engine poisoned")
            .plan_execution(
                session_id,
                parallel,
                sequential,
                estimated_rounds,
                confidence,
            )
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
                .unwrap_or_else(|| self.config.grok_model.clone()),
            system: Some("Answer concisely with factual claims grounded in web search sources. Prefer primary sources. If sources are weak or unavailable, say so.".to_string()),
            messages: vec![SearchMessage {
                role: "user".to_string(),
                content: vec![ContentBlock::text(content)],
            }],
            tools: vec![SearchTool::web_search()],
        }
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

fn with_provider(mut sources: Vec<Source>, provider: &str) -> Vec<Source> {
    for source in &mut sources {
        source.provider = provider.to_string();
    }
    sources
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
    async fn search_sources(&self, _query: &str, max_results: usize) -> Result<Vec<Source>> {
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
