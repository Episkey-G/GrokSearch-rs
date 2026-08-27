use std::collections::HashSet;
use std::io::Write;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::logging::DebugEvent;
use crate::model::search::SearchFilters;
use crate::model::tool::{WebSearchInput, WebSearchOutput};

const SCHEMA_VERSION: u32 = 1;
const FAILED_FETCH_PREFIX: &str = "_Failed to retrieve:";

/// Minimal, privacy-preserving request context retained for the shadow gate.
/// The raw query is deliberately not copied so it can never enter diagnostics.
#[derive(Debug, Clone)]
pub(super) struct ObservationInput {
    filters: SearchFilters,
    include_content: bool,
}

impl ObservationInput {
    /// `include_content` is supplied by the caller rather than re-derived here:
    /// `super::resolve_include_content` owns that contract, so observation can
    /// never disagree with the search it is observing.
    pub(super) fn new(input: &WebSearchInput, include_content: bool) -> Self {
        Self {
            filters: SearchFilters {
                recency_days: input.recency_days,
                include_domains: input.include_domains.clone(),
                exclude_domains: input.exclude_domains.clone(),
            },
            include_content,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum QualitySignal {
    AnswerEmpty,
    SourcesEmpty,
    NoValidHttpUrls,
    IncludeDomainsUnmet,
    ExcludedDomainPresent,
    AllInlineFetchesFailed,
    UpstreamUnverifiable,
    UpstreamProviderFailure,
    CanonicalEquivalentUrlsPresent,
    ExactDuplicateUrlsPresent,
    SingleHostResults,
    RecencyUnverifiable,
    AllTitlesMissing,
    SomeInlineFetchesFailed,
}

impl QualitySignal {
    const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::AnswerEmpty
                | Self::SourcesEmpty
                | Self::NoValidHttpUrls
                | Self::IncludeDomainsUnmet
                | Self::ExcludedDomainPresent
                | Self::AllInlineFetchesFailed
                | Self::UpstreamUnverifiable
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct QualityMetrics {
    cached_source_count: usize,
    visible_source_count: usize,
    valid_http_url_count: usize,
    invalid_or_unsupported_url_count: usize,
    empty_url_count: usize,
    unique_host_count: usize,
    unique_provider_count: usize,
    titled_source_count: usize,
    described_source_count: usize,
    dated_source_count: usize,
    inline_content_source_count: usize,
    inline_fetch_failure_count: usize,
    exact_duplicate_url_count: usize,
    canonical_equivalent_url_count: usize,
    include_domain_match_count: usize,
    excluded_domain_match_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct QualityReport {
    schema_version: u32,
    would_retry: bool,
    hard_failures: Vec<QualitySignal>,
    advisories: Vec<QualitySignal>,
    metrics: QualityMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UrlIdentity {
    key: String,
    host: Option<String>,
    valid_http: bool,
}

/// Produce a conservative identity without rewriting the URL returned to the
/// caller. Only normalization guaranteed by `url::Url` is observed. Scheme,
/// query, fragment, path case, trailing slash, userinfo, and non-default ports
/// remain part of the key; tracking parameters are never removed.
fn source_url_identity(raw: &str) -> Option<UrlIdentity> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    match url::Url::parse(trimmed) {
        Ok(parsed)
            if matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some() =>
        {
            Some(UrlIdentity {
                key: parsed.as_str().to_string(),
                host: parsed.host_str().map(str::to_ascii_lowercase),
                valid_http: true,
            })
        }
        _ => Some(UrlIdentity {
            key: trimmed.to_string(),
            host: None,
            valid_http: false,
        }),
    }
}

fn normalized_filter_domain(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches("*.").trim_start_matches('.');
    if trimmed.is_empty() {
        return None;
    }

    let parsed = if trimmed.contains("://") {
        url::Url::parse(trimmed).ok()
    } else {
        url::Url::parse(&format!("https://{trimmed}")).ok()
    }?;
    parsed.host_str().map(str::to_ascii_lowercase)
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn has_text(value: Option<&str>) -> bool {
    value.is_some_and(|text| !text.trim().is_empty())
}

fn evaluate(input: &ObservationInput, output: &WebSearchOutput) -> QualityReport {
    let include_domains: Vec<String> = input
        .filters
        .include_domains
        .iter()
        .filter_map(|domain| normalized_filter_domain(domain))
        .collect();
    let exclude_domains: Vec<String> = input
        .filters
        .exclude_domains
        .iter()
        .filter_map(|domain| normalized_filter_domain(domain))
        .collect();

    let mut exact_urls = HashSet::new();
    let mut canonical_urls = HashSet::new();
    let mut hosts = HashSet::new();
    let mut providers = HashSet::new();
    let mut metrics = QualityMetrics {
        cached_source_count: output.sources_count,
        visible_source_count: output.sources.len(),
        valid_http_url_count: 0,
        invalid_or_unsupported_url_count: 0,
        empty_url_count: 0,
        unique_host_count: 0,
        unique_provider_count: 0,
        titled_source_count: 0,
        described_source_count: 0,
        dated_source_count: 0,
        inline_content_source_count: 0,
        inline_fetch_failure_count: 0,
        exact_duplicate_url_count: 0,
        canonical_equivalent_url_count: 0,
        include_domain_match_count: 0,
        excluded_domain_match_count: 0,
    };

    for source in &output.sources {
        if has_text(source.title.as_deref()) {
            metrics.titled_source_count += 1;
        }
        if has_text(source.description.as_deref()) {
            metrics.described_source_count += 1;
        }
        if has_text(source.published_date.as_deref()) {
            metrics.dated_source_count += 1;
        }
        if let Some(content) = source
            .content
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            metrics.inline_content_source_count += 1;
            if content.trim_start().starts_with(FAILED_FETCH_PREFIX) {
                metrics.inline_fetch_failure_count += 1;
            }
        }
        providers.insert(source.provider.to_string());

        let Some(identity) = source_url_identity(&source.url) else {
            metrics.empty_url_count += 1;
            continue;
        };
        if !identity.valid_http {
            metrics.invalid_or_unsupported_url_count += 1;
            continue;
        }
        metrics.valid_http_url_count += 1;

        let trimmed = source.url.trim().to_string();
        let exact_duplicate = !exact_urls.insert(trimmed);
        let canonical_duplicate = !canonical_urls.insert(identity.key);
        if exact_duplicate {
            metrics.exact_duplicate_url_count += 1;
        } else if canonical_duplicate {
            metrics.canonical_equivalent_url_count += 1;
        }

        if let Some(host) = identity.host {
            if include_domains
                .iter()
                .any(|domain| host_matches_domain(&host, domain))
            {
                metrics.include_domain_match_count += 1;
            }
            if exclude_domains
                .iter()
                .any(|domain| host_matches_domain(&host, domain))
            {
                metrics.excluded_domain_match_count += 1;
            }
            hosts.insert(host);
        }
    }

    metrics.unique_host_count = hosts.len();
    metrics.unique_provider_count = providers.len();

    let mut hard_failures = Vec::new();
    let mut advisories = Vec::new();

    if output.content.trim().is_empty() {
        hard_failures.push(QualitySignal::AnswerEmpty);
    }
    if output.sources.is_empty() {
        hard_failures.push(QualitySignal::SourcesEmpty);
    } else if metrics.valid_http_url_count == 0 {
        hard_failures.push(QualitySignal::NoValidHttpUrls);
    }
    if !input.filters.include_domains.is_empty() && metrics.include_domain_match_count == 0 {
        hard_failures.push(QualitySignal::IncludeDomainsUnmet);
    }
    if metrics.excluded_domain_match_count > 0 {
        hard_failures.push(QualitySignal::ExcludedDomainPresent);
    }
    if input.include_content
        && metrics.inline_content_source_count > 0
        && metrics.inline_fetch_failure_count == metrics.inline_content_source_count
    {
        hard_failures.push(QualitySignal::AllInlineFetchesFailed);
    } else if metrics.inline_fetch_failure_count > 0 {
        advisories.push(QualitySignal::SomeInlineFetchesFailed);
    }

    match output.fallback_reason.as_deref() {
        Some("grok_content_empty" | "grok_sources_empty") => {
            hard_failures.push(QualitySignal::UpstreamUnverifiable);
        }
        Some(_) => hard_failures.push(QualitySignal::UpstreamProviderFailure),
        None => {}
    }

    if metrics.exact_duplicate_url_count > 0 {
        advisories.push(QualitySignal::ExactDuplicateUrlsPresent);
    }
    if metrics.canonical_equivalent_url_count > 0 {
        advisories.push(QualitySignal::CanonicalEquivalentUrlsPresent);
    }
    if metrics.valid_http_url_count > 1 && metrics.unique_host_count == 1 {
        advisories.push(QualitySignal::SingleHostResults);
    }
    if input.filters.recency_days.is_some()
        && metrics.valid_http_url_count > 0
        && metrics.dated_source_count == 0
    {
        advisories.push(QualitySignal::RecencyUnverifiable);
    }
    if metrics.valid_http_url_count > 0 && metrics.titled_source_count == 0 {
        advisories.push(QualitySignal::AllTitlesMissing);
    }

    let would_retry = hard_failures
        .iter()
        .copied()
        .any(QualitySignal::is_retryable);

    QualityReport {
        schema_version: SCHEMA_VERSION,
        would_retry,
        hard_failures,
        advisories,
        metrics,
    }
}

fn build_event(
    input: &ObservationInput,
    output: &WebSearchOutput,
    elapsed: Duration,
) -> DebugEvent {
    let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
    DebugEvent::new(
        "search_quality_shadow",
        json!({
            "session_id": output.session_id,
            "elapsed_ms": elapsed_ms,
            "answer_chars": output.content.chars().count(),
            "search_provider": output.search_provider,
            "fallback_used": output.fallback_used,
            "fallback_reason": output.fallback_reason,
            "truncated": output.truncated,
            "report": evaluate(input, output),
        }),
    )
}

/// Emit one compact JSON line to stderr. Shadow observation must never alter a
/// successful search, so serialization failure is intentionally ignored.
pub(super) fn observe(input: &ObservationInput, output: &WebSearchOutput, elapsed: Duration) {
    let event = build_event(input, output, elapsed);
    if let Ok(line) = serde_json::to_string(&event) {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::source::Source;

    fn output_with(sources: Vec<Source>) -> WebSearchOutput {
        WebSearchOutput {
            session_id: "session-test".to_string(),
            content: "answer".to_string(),
            sources_count: sources.len(),
            sources,
            search_provider: "grok_responses".to_string(),
            fallback_used: false,
            fallback_reason: None,
            truncated: false,
        }
    }

    fn observation() -> ObservationInput {
        ObservationInput {
            filters: SearchFilters::default(),
            include_content: false,
        }
    }

    #[test]
    fn canonical_identity_only_applies_url_standard_normalization() {
        let normalized = source_url_identity(" HTTPS://EXAMPLE.COM:443/a/../docs ").unwrap();
        let equivalent = source_url_identity("https://example.com/docs").unwrap();
        assert_eq!(normalized.key, equivalent.key);

        assert_ne!(
            source_url_identity("http://example.com/docs").unwrap().key,
            equivalent.key
        );
        assert_ne!(
            source_url_identity("https://example.com/docs#one")
                .unwrap()
                .key,
            source_url_identity("https://example.com/docs#two")
                .unwrap()
                .key
        );
        assert_ne!(
            source_url_identity("https://example.com/docs?utm_source=a")
                .unwrap()
                .key,
            equivalent.key
        );
    }

    #[test]
    fn one_authoritative_source_is_not_penalized_for_source_count() {
        let output = output_with(vec![Source::new(
            "https://docs.example.com/reference",
            "grok_responses",
        )
        .with_title("Reference")]);
        let report = evaluate(&observation(), &output);
        assert!(!report.would_retry);
        assert!(report.hard_failures.is_empty());
        assert!(!report
            .advisories
            .contains(&QualitySignal::SingleHostResults));
    }

    #[test]
    fn explicit_domain_violations_are_retry_candidates() {
        let input = ObservationInput {
            filters: SearchFilters {
                recency_days: None,
                include_domains: vec!["docs.example.com".to_string()],
                exclude_domains: vec!["spam.example".to_string()],
            },
            include_content: false,
        };
        let output = output_with(vec![Source::new(
            "https://spam.example/copied",
            "grok_responses",
        )]);
        let report = evaluate(&input, &output);
        assert!(report.would_retry);
        assert!(report
            .hard_failures
            .contains(&QualitySignal::IncludeDomainsUnmet));
        assert!(report
            .hard_failures
            .contains(&QualitySignal::ExcludedDomainPresent));
    }

    #[test]
    fn provider_failure_is_blocking_but_not_a_focused_retry_candidate() {
        let mut output = output_with(vec![Source::new(
            "https://example.com/source",
            "tavily_fallback",
        )]);
        output.fallback_used = true;
        output.fallback_reason = Some("grok_auth_error".to_string());
        output.search_provider = "source_fallback".to_string();

        let report = evaluate(&observation(), &output);
        assert!(!report.would_retry);
        assert!(report
            .hard_failures
            .contains(&QualitySignal::UpstreamProviderFailure));
    }

    #[test]
    fn event_never_contains_query_answer_url_or_filter_values() {
        let input = ObservationInput {
            filters: SearchFilters {
                recency_days: Some(7),
                include_domains: vec!["private-filter.example".to_string()],
                exclude_domains: Vec::new(),
            },
            include_content: true,
        };
        let mut output = output_with(vec![Source::new(
            "https://private-url.example/token-value",
            "grok_responses",
        )]);
        output.content = "private answer text".to_string();

        let serialized =
            serde_json::to_string(&build_event(&input, &output, Duration::from_millis(12)))
                .unwrap();
        assert!(!serialized.contains("private answer text"));
        assert!(!serialized.contains("private-url.example"));
        assert!(!serialized.contains("private-filter.example"));
        assert!(serialized.contains("search_quality_shadow"));
        assert!(serialized.contains("answer_chars"));
    }
}
