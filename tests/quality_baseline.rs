use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use grok_search_rs::error::{GrokSearchError, Result};
use grok_search_rs::model::search::{SearchFilters, SearchRequest, SearchResponse};
use grok_search_rs::model::source::{FetchedPage, Source};
use grok_search_rs::model::tool::{WebSearchInput, WebSearchOutput};
use grok_search_rs::service::{AiProvider, SearchService, SourceProvider};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct QualityFixture {
    schema_version: u32,
    as_of: String,
    cases: Vec<QualityCase>,
}

#[derive(Debug, Deserialize)]
struct QualityCase {
    id: String,
    input: FixtureInput,
    ai_answer: String,
    ai_sources: Vec<FixtureSource>,
    supplemental_sources: Vec<FixtureSource>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct FixtureInput {
    query: String,
    extra_sources: usize,
    #[serde(default)]
    recency_days: Option<u32>,
    #[serde(default)]
    include_domains: Vec<String>,
    #[serde(default)]
    exclude_domains: Vec<String>,
    include_content: bool,
}

#[derive(Debug, Deserialize)]
struct FixtureSource {
    url: String,
    provider: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    published_date: Option<String>,
}

impl FixtureSource {
    fn to_source(&self) -> Source {
        let mut source = Source::new(self.url.clone(), self.provider.clone());
        source.title.clone_from(&self.title);
        source.description.clone_from(&self.description);
        source.published_date.clone_from(&self.published_date);
        source
    }
}

#[derive(Debug, Deserialize)]
struct Expected {
    answer_contains: Vec<String>,
    required_domain_groups: Vec<RequiredDomainGroup>,
    min_unique_hosts: usize,
    expected_source_count: usize,
    #[serde(default)]
    min_published_date: Option<String>,
    #[serde(default)]
    first_source_provider: Option<String>,
    #[serde(default)]
    first_source_title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RequiredDomainGroup {
    domains: Vec<String>,
    top_k: usize,
}

#[derive(Clone)]
struct ScriptedAiProvider {
    response: SearchResponse,
    requests: Arc<Mutex<Vec<SearchRequest>>>,
}

#[async_trait]
impl AiProvider for ScriptedAiProvider {
    async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
        self.requests
            .lock()
            .expect("AI request lock")
            .push(request.clone());
        Ok(self.response.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceCall {
    query: String,
    max_results: usize,
    filters: SearchFilters,
}

#[derive(Clone)]
struct RecordingSourceProvider {
    sources: Vec<Source>,
    calls: Arc<Mutex<Vec<SourceCall>>>,
}

#[async_trait]
impl SourceProvider for RecordingSourceProvider {
    async fn search_sources(
        &self,
        query: &str,
        max_results: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<Source>> {
        self.calls
            .lock()
            .expect("source call lock")
            .push(SourceCall {
                query: query.to_string(),
                max_results,
                filters: filters.clone(),
            });
        Ok(self.sources.iter().take(max_results).cloned().collect())
    }

    async fn fetch(&self, _url: &str) -> Result<FetchedPage> {
        Err(GrokSearchError::Provider(
            "quality fixture unexpectedly fetched inline content".to_string(),
        ))
    }

    async fn map(&self, _url: &str, _max_results: usize) -> Result<Vec<Source>> {
        Ok(Vec::new())
    }
}

struct RunEvidence {
    output: WebSearchOutput,
    ai_requests: Vec<SearchRequest>,
    source_calls: Vec<SourceCall>,
}

async fn run_case(case: &QualityCase) -> RunEvidence {
    run_case_with_shadow(case, false).await
}

async fn run_case_with_shadow(case: &QualityCase, shadow: bool) -> RunEvidence {
    let ai_requests = Arc::new(Mutex::new(Vec::new()));
    let source_calls = Arc::new(Mutex::new(Vec::new()));
    let ai = ScriptedAiProvider {
        response: SearchResponse {
            content: case.ai_answer.clone(),
            sources: case
                .ai_sources
                .iter()
                .map(FixtureSource::to_source)
                .collect(),
        },
        requests: ai_requests.clone(),
    };
    let source_provider = RecordingSourceProvider {
        sources: case
            .supplemental_sources
            .iter()
            .map(FixtureSource::to_source)
            .collect(),
        calls: source_calls.clone(),
    };
    let service = SearchService::fake_custom(
        Some(Arc::new(ai)),
        Arc::new(source_provider),
        None,
        [(
            "GROK_SEARCH_QUALITY_GATE_SHADOW",
            if shadow { "true" } else { "false" },
        )],
    );
    let output = service
        .web_search(WebSearchInput {
            query: case.input.query.clone(),
            extra_sources: Some(case.input.extra_sources),
            recency_days: case.input.recency_days,
            include_domains: case.input.include_domains.clone(),
            exclude_domains: case.input.exclude_domains.clone(),
            include_content: Some(case.input.include_content),
            ..Default::default()
        })
        .await
        .unwrap_or_else(|err| panic!("quality case {} failed: {err}", case.id));

    let recorded_ai = ai_requests.lock().expect("AI request lock").clone();
    let recorded_sources = source_calls.lock().expect("source call lock").clone();
    RunEvidence {
        output,
        ai_requests: recorded_ai,
        source_calls: recorded_sources,
    }
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn output_without_session(output: &WebSearchOutput) -> impl PartialEq + '_ {
    (
        &output.content,
        output.sources_count,
        &output.sources,
        &output.search_provider,
        output.fallback_used,
        &output.fallback_reason,
        output.truncated,
    )
}

fn assert_case(case: &QualityCase, evidence: &RunEvidence) {
    let output = &evidence.output;
    assert!(
        !output.content.trim().is_empty(),
        "{}: empty answer",
        case.id
    );
    assert!(!output.fallback_used, "{}: unexpected fallback", case.id);
    assert_eq!(
        output.sources_count, case.expected.expected_source_count,
        "{}: source count",
        case.id
    );
    assert_eq!(
        output.sources_count,
        output.sources.len(),
        "{}: fixture responses must fit the response budget",
        case.id
    );
    assert!(
        output
            .sources
            .iter()
            .all(|source| url::Url::parse(&source.url)
                .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))),
        "{}: every source must be a valid HTTP(S) URL",
        case.id
    );
    for token in &case.expected.answer_contains {
        assert!(
            output.content.contains(token),
            "{}: answer missing token {token:?}",
            case.id
        );
    }
    assert!(
        !output.content.contains('\u{fffd}')
            && output.sources.iter().all(|source| !source
                .title
                .as_deref()
                .unwrap_or_default()
                .contains('\u{fffd}')),
        "{}: replacement character indicates UTF-8 corruption",
        case.id
    );

    let hosts: HashSet<String> = output
        .sources
        .iter()
        .filter_map(|source| url::Url::parse(&source.url).ok())
        .filter_map(|url| url.host_str().map(str::to_ascii_lowercase))
        .collect();
    assert!(
        hosts.len() >= case.expected.min_unique_hosts,
        "{}: expected at least {} unique hosts, got {hosts:?}",
        case.id,
        case.expected.min_unique_hosts
    );

    for group in &case.expected.required_domain_groups {
        let matched = output
            .sources
            .iter()
            .take(group.top_k)
            .filter_map(|source| url::Url::parse(&source.url).ok())
            .filter_map(|url| url.host_str().map(str::to_ascii_lowercase))
            .any(|host| {
                group
                    .domains
                    .iter()
                    .any(|domain| host_matches_domain(&host, domain))
            });
        assert!(
            matched,
            "{}: no required domain {:?} in top {}",
            case.id, group.domains, group.top_k
        );
    }

    if let Some(cutoff) = &case.expected.min_published_date {
        assert!(
            output
                .sources
                .iter()
                .filter_map(|source| source.published_date.as_deref())
                .any(|date| date >= cutoff.as_str()),
            "{}: no source at or after fixed cutoff {cutoff}",
            case.id
        );
    }
    if let Some(provider) = &case.expected.first_source_provider {
        assert_eq!(
            output
                .sources
                .first()
                .map(|source| source.provider.as_ref()),
            Some(provider.as_str()),
            "{}: first source provider",
            case.id
        );
    }
    if let Some(title) = &case.expected.first_source_title {
        assert_eq!(
            output
                .sources
                .first()
                .and_then(|source| source.title.as_deref()),
            Some(title.as_str()),
            "{}: first source title",
            case.id
        );
    }

    assert_eq!(evidence.ai_requests.len(), 1, "{}: AI calls", case.id);
    let request = &evidence.ai_requests[0];
    assert!(
        request
            .system
            .as_deref()
            .is_some_and(|system| system.contains("Prefer primary sources")),
        "{}: primary-source instruction missing",
        case.id
    );
    let user_text = request.messages[0].content[0].as_text();
    assert!(
        user_text.contains(&case.input.query),
        "{}: original query was not preserved",
        case.id
    );
    if let Some(days) = case.input.recency_days {
        assert!(
            user_text.contains(&format!("last {days} day(s)")),
            "{}: recency prompt missing",
            case.id
        );
    }

    assert_eq!(evidence.source_calls.len(), 1, "{}: source calls", case.id);
    let source_call = &evidence.source_calls[0];
    assert_eq!(
        source_call.query, case.input.query,
        "{}: raw query",
        case.id
    );
    assert_eq!(
        source_call.filters,
        SearchFilters {
            recency_days: case.input.recency_days,
            include_domains: case.input.include_domains.clone(),
            exclude_domains: case.input.exclude_domains.clone(),
        },
        "{}: structured filters",
        case.id
    );
    assert!(
        source_call.max_results >= case.input.extra_sources,
        "{}: speculative source budget must cover requested enrichment",
        case.id
    );
}

fn load_fixture() -> QualityFixture {
    serde_json::from_str(include_str!("fixtures/quality/cases.json"))
        .expect("quality fixture must parse")
}

#[tokio::test]
async fn deterministic_quality_baseline_cases_pass_without_live_providers() {
    let fixture = load_fixture();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.as_of, "2026-07-22");
    assert_eq!(fixture.cases.len(), 5, "baseline scenario count changed");

    for case in &fixture.cases {
        let first = run_case(case).await;
        assert_case(case, &first);

        let second = run_case(case).await;
        assert_case(case, &second);
        assert!(
            output_without_session(&first.output) == output_without_session(&second.output),
            "{}: output changed across identical offline runs",
            case.id
        );
    }
}

/// Shadow observation is opt-in and must stay observation-only: enabling it may
/// not add an upstream call, rewrite the query, or change the returned result.
#[tokio::test]
async fn shadow_observation_never_changes_the_search_it_observes() {
    let fixture = load_fixture();

    for case in &fixture.cases {
        let off = run_case_with_shadow(case, false).await;
        let on = run_case_with_shadow(case, true).await;

        assert_case(case, &on);
        assert!(
            output_without_session(&off.output) == output_without_session(&on.output),
            "{}: shadow observation changed the user-visible result",
            case.id
        );
        assert_eq!(
            on.ai_requests.len(),
            off.ai_requests.len(),
            "{}: shadow observation issued an extra AI call",
            case.id
        );
        assert_eq!(
            on.source_calls, off.source_calls,
            "{}: shadow observation changed the source query or filters",
            case.id
        );
    }
}
