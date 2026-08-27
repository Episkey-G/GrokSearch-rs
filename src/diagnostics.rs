use serde::Serialize;
use serde_json::Value;

use crate::config::{
    credential_is_set as is_set, AuthMode, Config, ConfigFileState, LoadedConfig, Transport,
};
use crate::error::GrokSearchError;
use crate::service::SearchService;

pub const DOCTOR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Ready,
    Degraded,
    NotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPresence {
    Set,
    Unset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialSummary {
    pub grok_api_key: CredentialPresence,
    pub openai_compatible_api_key: CredentialPresence,
    pub tavily_api_key: CredentialPresence,
    pub firecrawl_api_key: CredentialPresence,
    pub github_token: CredentialPresence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigSummary {
    pub file_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub transport: String,
    pub auth_mode: String,
    pub base_url: String,
    pub model: String,
    pub tavily_base_url: String,
    pub firecrawl_base_url: String,
    pub timeout_seconds: u64,
    pub credentials: CredentialSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    pub id: String,
    pub status: CheckStatus,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub status: DoctorStatus,
    /// `true` means the core AI search path is usable. A report may therefore
    /// be `degraded` but still `ok` when only an optional source provider is
    /// unavailable.
    pub ok: bool,
    pub version: String,
    pub config: ConfigSummary,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionalProbePlan {
    Live,
    Disabled,
    Missing,
    InvalidUrl,
}

/// Produce a safe CLI diagnostic report. Configuration preflight always runs
/// first and never requires provider construction. Live probes are delegated to
/// `SearchService::doctor()` only when the core configuration is complete and
/// its active base URL is safe and valid.
pub async fn diagnose(loaded: LoadedConfig) -> DoctorReport {
    let LoadedConfig {
        config,
        config_path,
        file_state,
        ..
    } = loaded;

    let mut checks = Vec::new();
    let mut degraded = false;
    let file_state_label = file_state.as_str().to_string();
    let (file_check, file_degraded) = config_file_check(&file_state);
    checks.push(file_check);
    degraded |= file_degraded;

    let summary = config_summary(&config, file_state_label, config_path.as_deref());
    let grok_key_set = is_set(&config.grok_api_key);
    let compat_url_set = is_set(&config.openai_compatible_api_url);
    let compat_key_set = is_set(&config.openai_compatible_api_key);
    let partial_compat = compat_url_set != compat_key_set;

    // A partial inactive compatible configuration is still worth surfacing:
    // Grok may remain usable, but the user's intended alternative transport is
    // not. It is a degradation rather than a core failure in that case.
    if partial_compat && (grok_key_set || config.grok_auth_mode == AuthMode::OAuth) {
        checks.push(check(
            "openai_compatible_config",
            CheckStatus::Failed,
            "partial_openai_compatible_config",
            "OpenAI-compatible transport is only partially configured.",
            Some(
                "Set both OPENAI_COMPATIBLE_API_URL and OPENAI_COMPATIBLE_API_KEY, or remove both."
                    .to_string(),
            ),
        ));
        degraded = true;
    }

    let core_preflight = core_preflight_check(&config, partial_compat);
    let tavily_plan = optional_probe_plan(
        config.tavily_enabled,
        &config.tavily_api_key,
        &config.tavily_api_url,
    );
    let firecrawl_plan = optional_probe_plan(
        config.firecrawl_enabled,
        &config.firecrawl_api_key,
        &config.firecrawl_api_url,
    );

    if let Some(core_failure) = core_preflight {
        checks.push(core_failure);
        checks.push(optional_check_without_live(
            "tavily",
            "Tavily",
            tavily_plan,
            "Set TAVILY_API_KEY to enable source search, fetch, and map.",
            true,
        ));
        checks.push(optional_check_without_live(
            "firecrawl",
            "Firecrawl",
            firecrawl_plan,
            "Set FIRECRAWL_API_KEY to enable the optional fetch fallback.",
            true,
        ));
        return finish_report(summary, checks, false, true);
    }

    // Invalid optional endpoints must not reach reqwest. Disable just those
    // providers (and missing/disabled ones) in the clone used for the shared
    // live probe; their explicit checks remain in the final report.
    let mut probe_config = config.clone();
    if !is_set(&probe_config.grok_api_key) {
        probe_config.grok_api_key = None;
    }
    if !is_set(&probe_config.openai_compatible_api_key) {
        probe_config.openai_compatible_api_key = None;
    }
    if tavily_plan != OptionalProbePlan::Live {
        probe_config.tavily_enabled = false;
        probe_config.tavily_api_key = None;
    }
    if firecrawl_plan != OptionalProbePlan::Live {
        probe_config.firecrawl_enabled = false;
        probe_config.firecrawl_api_key = None;
    }

    match SearchService::new(probe_config) {
        Ok(service) => {
            let live = service.doctor().await;
            let ai_check = live_provider_check(
                "ai",
                active_provider_label(&config),
                live.get("grok"),
                "Verify the active API key or OAuth login, base URL, and model, then rerun doctor.",
            );
            let core_ok = ai_check.status == CheckStatus::Passed;
            checks.push(ai_check);

            let (tavily_check, tavily_degraded) = optional_check_from_live(
                "tavily",
                "Tavily",
                tavily_plan,
                live.get("tavily"),
                "Set TAVILY_API_KEY to enable source search, fetch, and map.",
            );
            checks.push(tavily_check);
            degraded |= tavily_degraded;

            let (firecrawl_check, firecrawl_degraded) = optional_check_from_live(
                "firecrawl",
                "Firecrawl",
                firecrawl_plan,
                live.get("firecrawl"),
                "Set FIRECRAWL_API_KEY to enable the optional fetch fallback.",
            );
            checks.push(firecrawl_check);
            degraded |= firecrawl_degraded;

            finish_report(summary, checks, core_ok, degraded)
        }
        Err(error) => {
            checks.push(service_construction_check(&error));
            checks.push(optional_check_without_live(
                "tavily",
                "Tavily",
                tavily_plan,
                "Set TAVILY_API_KEY to enable source search, fetch, and map.",
                true,
            ));
            checks.push(optional_check_without_live(
                "firecrawl",
                "Firecrawl",
                firecrawl_plan,
                "Set FIRECRAWL_API_KEY to enable the optional fetch fallback.",
                true,
            ));
            finish_report(summary, checks, false, true)
        }
    }
}

pub fn render_text(report: &DoctorReport) -> String {
    let status = match report.status {
        DoctorStatus::Ready => "READY",
        DoctorStatus::Degraded => "DEGRADED",
        DoctorStatus::NotReady => "NOT READY",
    };
    let mut output = format!(
        "grok-search-rs doctor v{}\nStatus: {}\nTransport: {}\nBase URL: {}\nModel: {}\nConfig file: {}{}\n\nChecks:\n",
        report.version,
        status,
        terminal_safe(&report.config.transport),
        terminal_safe(&report.config.base_url),
        terminal_safe(&report.config.model),
        terminal_safe(&report.config.file_state),
        report
            .config
            .config_path
            .as_deref()
            .map(|path| format!(" ({})", terminal_safe(path)))
            .unwrap_or_default(),
    );
    for item in &report.checks {
        let marker = match item.status {
            CheckStatus::Passed => "PASS",
            CheckStatus::Failed => "FAIL",
            CheckStatus::Skipped => "SKIP",
        };
        output.push_str(&format!(
            "  [{marker}] {}: {} ({})\n",
            item.id, item.message, item.code
        ));
        if let Some(action) = &item.action {
            output.push_str(&format!("         Action: {action}\n"));
        }
    }
    output
}

impl DoctorReport {
    pub fn render_text(&self) -> String {
        render_text(self)
    }
}

fn finish_report(
    config: ConfigSummary,
    checks: Vec<DoctorCheck>,
    core_ok: bool,
    degraded: bool,
) -> DoctorReport {
    let status = if !core_ok {
        DoctorStatus::NotReady
    } else if degraded {
        DoctorStatus::Degraded
    } else {
        DoctorStatus::Ready
    };
    DoctorReport {
        schema_version: DOCTOR_SCHEMA_VERSION,
        status,
        ok: core_ok,
        version: env!("CARGO_PKG_VERSION").to_string(),
        config,
        checks,
    }
}

fn config_summary(
    config: &Config,
    file_state: String,
    config_path: Option<&std::path::Path>,
) -> ConfigSummary {
    let (transport, active_url, model) = match config.transport {
        Transport::Responses => (
            "grok_responses",
            config.grok_api_url.as_str(),
            config.grok_model.as_str(),
        ),
        Transport::ChatCompletions => (
            "openai_compatible",
            config.openai_compatible_api_url.as_deref().unwrap_or(""),
            config
                .openai_compatible_model
                .as_deref()
                .unwrap_or(config.grok_model.as_str()),
        ),
    };
    ConfigSummary {
        file_state,
        config_path: config_path.map(|path| bounded_text(&path.display().to_string(), 512)),
        transport: transport.to_string(),
        auth_mode: match config.grok_auth_mode {
            AuthMode::ApiKey => "api_key".to_string(),
            AuthMode::OAuth => "oauth".to_string(),
        },
        base_url: safe_base_url(active_url),
        model: bounded_text(model, 256),
        tavily_base_url: safe_base_url(&config.tavily_api_url),
        firecrawl_base_url: safe_base_url(&config.firecrawl_api_url),
        timeout_seconds: config.timeout.as_secs(),
        credentials: CredentialSummary {
            grok_api_key: presence(&config.grok_api_key),
            openai_compatible_api_key: presence(&config.openai_compatible_api_key),
            tavily_api_key: presence(&config.tavily_api_key),
            firecrawl_api_key: presence(&config.firecrawl_api_key),
            github_token: presence(&config.github_token),
        },
    }
}

fn config_file_check(state: &ConfigFileState) -> (DoctorCheck, bool) {
    match state {
        ConfigFileState::Loaded => (
            check(
                "config_file",
                CheckStatus::Passed,
                "config_file_loaded",
                "Configuration file loaded.",
                None,
            ),
            false,
        ),
        ConfigFileState::Missing => (
            check(
                "config_file",
                CheckStatus::Skipped,
                "config_file_missing",
                "No configuration file was found; environment variables still apply.",
                Some(
                    "Run `grok-search-rs setup` (recommended), or use `grok-search-rs --init` for a manual template."
                        .to_string(),
                ),
            ),
            false,
        ),
        ConfigFileState::Unresolved => (
            check(
                "config_file",
                CheckStatus::Skipped,
                "config_file_unresolved",
                "No configuration file path could be resolved; environment variables still apply.",
                Some(
                    "Set GROK_SEARCH_CONFIG, or set HOME/USERPROFILE so the default path can be resolved."
                        .to_string(),
                ),
            ),
            false,
        ),
        ConfigFileState::Malformed => (
            check(
                "config_file",
                CheckStatus::Failed,
                "invalid_config_file",
                "The configuration file is malformed and was not applied.",
                Some("Fix the TOML syntax and unknown fields, then rerun doctor.".to_string()),
            ),
            true,
        ),
        ConfigFileState::Unreadable => (
            check(
                "config_file",
                CheckStatus::Failed,
                "invalid_config_file",
                "The configuration file could not be read and was not applied.",
                Some("Check file existence and permissions, then rerun doctor.".to_string()),
            ),
            true,
        ),
    }
}

fn core_preflight_check(config: &Config, partial_compat: bool) -> Option<DoctorCheck> {
    if !config.has_ai_credential() {
        if partial_compat {
            return Some(check(
                "ai",
                CheckStatus::Failed,
                "partial_openai_compatible_config",
                "OpenAI-compatible transport is only partially configured.",
                Some(
                    "Set both OPENAI_COMPATIBLE_API_URL and OPENAI_COMPATIBLE_API_KEY, or set GROK_SEARCH_API_KEY."
                        .to_string(),
                ),
            ));
        }
        return Some(check(
            "ai",
            CheckStatus::Failed,
            "missing_credentials",
            "No usable AI provider credential is configured.",
            Some(
                "Run `grok-search-rs setup` (recommended), set GROK_SEARCH_API_KEY, run `grok-search-rs login` for OAuth, or set both OPENAI_COMPATIBLE_API_URL and OPENAI_COMPATIBLE_API_KEY."
                    .to_string(),
            ),
        ));
    }

    let active_url = match config.transport {
        Transport::Responses => config.grok_api_url.as_str(),
        Transport::ChatCompletions => config.openai_compatible_api_url.as_deref().unwrap_or(""),
    };
    if validated_base_url(active_url).is_none() {
        return Some(check(
            "ai",
            CheckStatus::Failed,
            "invalid_base_url",
            "The active AI provider base URL is not a safe HTTP(S) base URL.",
            Some(
                "Set a valid http:// or https:// base URL without userinfo, query parameters, or fragments."
                    .to_string(),
            ),
        ));
    }
    None
}

fn optional_probe_plan(enabled: bool, key: &Option<String>, base_url: &str) -> OptionalProbePlan {
    if !enabled {
        OptionalProbePlan::Disabled
    } else if !is_set(key) {
        OptionalProbePlan::Missing
    } else if validated_base_url(base_url).is_none() {
        OptionalProbePlan::InvalidUrl
    } else {
        OptionalProbePlan::Live
    }
}

fn optional_check_from_live(
    id: &str,
    label: &str,
    plan: OptionalProbePlan,
    value: Option<&Value>,
    missing_action: &str,
) -> (DoctorCheck, bool) {
    match plan {
        OptionalProbePlan::Live => {
            let item = live_provider_check(id, label, value, missing_action);
            let degraded = item.status != CheckStatus::Passed;
            (item, degraded)
        }
        OptionalProbePlan::Disabled => (
            check(
                id,
                CheckStatus::Skipped,
                "provider_disabled",
                &format!("{label} is disabled."),
                None,
            ),
            false,
        ),
        OptionalProbePlan::Missing => (
            check(
                id,
                CheckStatus::Skipped,
                "not_configured",
                &format!("{label} is not configured."),
                Some(missing_action.to_string()),
            ),
            false,
        ),
        OptionalProbePlan::InvalidUrl => (invalid_optional_url_check(id, label), true),
    }
}

fn optional_check_without_live(
    id: &str,
    label: &str,
    plan: OptionalProbePlan,
    missing_action: &str,
    core_incomplete: bool,
) -> DoctorCheck {
    match plan {
        OptionalProbePlan::Live => check(
            id,
            CheckStatus::Skipped,
            "probe_not_run",
            &format!("{label} is configured but was not probed."),
            core_incomplete
                .then(|| "Fix the core AI configuration, then rerun doctor.".to_string()),
        ),
        OptionalProbePlan::Disabled => check(
            id,
            CheckStatus::Skipped,
            "provider_disabled",
            &format!("{label} is disabled."),
            None,
        ),
        OptionalProbePlan::Missing => check(
            id,
            CheckStatus::Skipped,
            "not_configured",
            &format!("{label} is not configured."),
            Some(missing_action.to_string()),
        ),
        OptionalProbePlan::InvalidUrl => invalid_optional_url_check(id, label),
    }
}

fn invalid_optional_url_check(id: &str, label: &str) -> DoctorCheck {
    check(
        id,
        CheckStatus::Failed,
        "invalid_base_url",
        &format!("{label} base URL is not a safe HTTP(S) base URL."),
        Some(
            "Set a valid http:// or https:// base URL without userinfo, query parameters, or fragments."
                .to_string(),
        ),
    )
}

fn live_provider_check(
    id: &str,
    label: &str,
    value: Option<&Value>,
    default_action: &str,
) -> DoctorCheck {
    let reachable = value
        .and_then(|provider| provider.get("reachable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if reachable {
        return check(
            id,
            CheckStatus::Passed,
            "provider_reachable",
            &format!("{label} is reachable."),
            None,
        );
    }
    let detail = value
        .and_then(|provider| provider.get("detail"))
        .and_then(Value::as_str)
        .unwrap_or("");
    classified_provider_failure(id, label, detail, default_action)
}

/// Classify raw provider detail into a fixed public vocabulary. The raw string
/// is intentionally used only for matching and is never copied into the report.
fn classified_provider_failure(
    id: &str,
    label: &str,
    detail: &str,
    default_action: &str,
) -> DoctorCheck {
    let lowered = detail.to_ascii_lowercase();
    if let Some(status) = provider_http_status(&lowered) {
        return match status {
            401 | 403 => check(
                id,
                CheckStatus::Failed,
                "invalid_credentials",
                &format!("{label} rejected the configured credentials."),
                Some(default_action.to_string()),
            ),
            429 | 432 | 433 => check(
                id,
                CheckStatus::Failed,
                "rate_limited",
                &format!("{label} rate-limited the diagnostic request."),
                Some(
                    "Wait for the provider limit to reset or use another valid key, then retry."
                        .to_string(),
                ),
            ),
            404 if lowered.contains("model") => check(
                id,
                CheckStatus::Failed,
                "model_not_available",
                &format!("{label} does not provide the configured model."),
                Some(
                    "Set the active model to one supported by the provider, then rerun doctor."
                        .to_string(),
                ),
            ),
            408 | 504 => check(
                id,
                CheckStatus::Failed,
                "provider_timeout",
                &format!("{label} did not respond before the timeout."),
                Some(
                    "Check network reachability and the configured base URL, then retry."
                        .to_string(),
                ),
            ),
            _ => check(
                id,
                CheckStatus::Failed,
                "provider_unreachable",
                &format!("{label} could not be reached or returned an unusable response."),
                Some(default_action.to_string()),
            ),
        };
    }
    if lowered.contains("timed out") || lowered.contains("timeout") {
        return check(
            id,
            CheckStatus::Failed,
            "provider_timeout",
            &format!("{label} did not respond before the timeout."),
            Some("Check network reachability and the configured base URL, then retry.".to_string()),
        );
    }
    if lowered.contains("429")
        || lowered.contains("432")
        || lowered.contains("433")
        || lowered.contains("rate limit")
        || lowered.contains("too many requests")
    {
        return check(
            id,
            CheckStatus::Failed,
            "rate_limited",
            &format!("{label} rate-limited the diagnostic request."),
            Some(
                "Wait for the provider limit to reset or use another valid key, then retry."
                    .to_string(),
            ),
        );
    }
    if lowered.contains("model")
        && (lowered.contains("not found")
            || lowered.contains("does not exist")
            || lowered.contains("unsupported")
            || lowered.contains("404"))
    {
        return check(
            id,
            CheckStatus::Failed,
            "model_not_available",
            &format!("{label} does not provide the configured model."),
            Some(
                "Set the active model to one supported by the provider, then rerun doctor."
                    .to_string(),
            ),
        );
    }
    if lowered.contains("401")
        || lowered.contains("403")
        || lowered.contains("unauthorized")
        || lowered.contains("forbidden")
        || lowered.contains("invalid api key")
        || lowered.contains("invalid_api_key")
        || lowered.contains("oauth_not_logged_in")
        || lowered.contains("credential")
    {
        return check(
            id,
            CheckStatus::Failed,
            "invalid_credentials",
            &format!("{label} rejected the configured credentials."),
            Some(default_action.to_string()),
        );
    }
    check(
        id,
        CheckStatus::Failed,
        "provider_unreachable",
        &format!("{label} could not be reached or returned an unusable response."),
        Some(default_action.to_string()),
    )
}

fn provider_http_status(detail: &str) -> Option<u16> {
    let (_, suffix) = detail.split_once("returned http ")?;
    suffix.split_whitespace().next()?.parse().ok()
}

fn service_construction_check(error: &GrokSearchError) -> DoctorCheck {
    match error {
        GrokSearchError::MissingConfig(_) => check(
            "ai",
            CheckStatus::Failed,
            "missing_credentials",
            "The AI provider could not be constructed because required configuration is missing.",
            Some("Set the required provider URL and credential, then rerun doctor.".to_string()),
        ),
        GrokSearchError::OAuth(_) => check(
            "ai",
            CheckStatus::Failed,
            "invalid_credentials",
            "OAuth credentials are unavailable or invalid.",
            Some(
                "Run `grok-search-rs login`, or set GROK_SEARCH_AUTH_FILE to a readable auth file."
                    .to_string(),
            ),
        ),
        GrokSearchError::Timeout(_) => check(
            "ai",
            CheckStatus::Failed,
            "provider_timeout",
            "The AI provider setup timed out.",
            Some("Check network reachability and the configured base URL, then retry.".to_string()),
        ),
        _ => check(
            "ai",
            CheckStatus::Failed,
            "provider_unreachable",
            "The AI provider could not be initialized.",
            Some(
                "Verify the provider configuration and credentials, then rerun doctor.".to_string(),
            ),
        ),
    }
}

fn check(
    id: &str,
    status: CheckStatus,
    code: &str,
    message: &str,
    action: Option<String>,
) -> DoctorCheck {
    DoctorCheck {
        id: id.to_string(),
        status,
        code: code.to_string(),
        message: message.to_string(),
        action,
    }
}

fn active_provider_label(config: &Config) -> &'static str {
    match config.transport {
        Transport::Responses => "Grok Responses",
        Transport::ChatCompletions => "OpenAI-compatible provider",
    }
}

fn presence(value: &Option<String>) -> CredentialPresence {
    if is_set(value) {
        CredentialPresence::Set
    } else {
        CredentialPresence::Unset
    }
}

fn safe_base_url(raw: &str) -> String {
    validated_base_url(raw).unwrap_or_else(|| "<invalid>".to_string())
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut output: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        output.push('…');
    }
    output
}

fn terminal_safe(value: &str) -> String {
    let mut output = String::new();
    for ch in bounded_text(value, 512).chars() {
        if ch.is_control() {
            output.extend(ch.escape_default());
        } else {
            output.push(ch);
        }
    }
    output
}

fn validated_base_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn loaded(config: Config, file_state: ConfigFileState) -> LoadedConfig {
        LoadedConfig {
            config,
            config_path: None,
            file_state,
            file_issue: None,
        }
    }

    fn has_code(report: &DoctorReport, code: &str) -> bool {
        report.checks.iter().any(|item| item.code == code)
    }

    #[tokio::test]
    async fn zero_config_returns_actionable_not_ready_report_without_probe() {
        let config = Config::from_env_map(Vec::<(String, String)>::new());
        let report = diagnose(loaded(config, ConfigFileState::Missing)).await;

        assert_eq!(report.schema_version, 1);
        assert_eq!(report.status, DoctorStatus::NotReady);
        assert!(!report.ok);
        assert!(has_code(&report, "missing_credentials"));
        assert!(report
            .checks
            .iter()
            .any(|item| item.status == CheckStatus::Skipped));
    }

    #[tokio::test]
    async fn malformed_and_unreadable_config_states_are_reported_not_panicked() {
        for (state, expected) in [
            (ConfigFileState::Malformed, "invalid_config_file"),
            (ConfigFileState::Unreadable, "invalid_config_file"),
        ] {
            let config = Config::from_env_map(Vec::<(String, String)>::new());
            let report = diagnose(loaded(config, state)).await;
            assert!(has_code(&report, expected));
            assert_eq!(report.status, DoctorStatus::NotReady);
        }
    }

    #[tokio::test]
    async fn partial_openai_compatible_config_names_the_real_problem() {
        for config in [
            Config::from_env_map([("OPENAI_COMPATIBLE_API_URL", "https://example.com/v1")]),
            Config::from_env_map([("OPENAI_COMPATIBLE_API_KEY", "sk-canary")]),
        ] {
            let report = diagnose(loaded(config, ConfigFileState::Unresolved)).await;
            assert!(has_code(&report, "partial_openai_compatible_config"));
            assert!(!report.ok);
            assert!(!serde_json::to_string(&report)
                .unwrap()
                .contains("sk-canary"));
        }
    }

    #[tokio::test]
    async fn invalid_or_secret_bearing_url_is_never_echoed_or_probed() {
        let mut config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "xai-super-secret"),
            (
                "GROK_SEARCH_URL",
                "https://alice:password@example.com/v1?token=query-secret#fragment",
            ),
            ("TAVILY_API_KEY", "tvly-super-secret"),
            ("FIRECRAWL_API_KEY", "fc-super-secret"),
            ("GITHUB_TOKEN", "ghp-super-secret"),
        ]);
        config.grok_auth_file = Some(PathBuf::from("/private/home/alice/secret-auth.json"));

        let report = diagnose(loaded(config, ConfigFileState::Loaded)).await;
        assert!(has_code(&report, "invalid_base_url"));
        let json = serde_json::to_string(&report).unwrap();
        let text = render_text(&report);
        for secret in [
            "xai-super-secret",
            "tvly-super-secret",
            "fc-super-secret",
            "ghp-super-secret",
            "alice:password",
            "query-secret",
            "secret-auth.json",
        ] {
            assert!(!json.contains(secret), "secret leaked in JSON: {secret}");
            assert!(!text.contains(secret), "secret leaked in text: {secret}");
        }
        assert_eq!(report.config.base_url, "<invalid>");
    }

    #[test]
    fn provider_failure_classification_is_stable_and_drops_raw_detail() {
        let cases = [
            ("request timed out with secret-body", "provider_timeout"),
            ("HTTP 429 secret-body", "rate_limited"),
            ("HTTP 401 secret-body", "invalid_credentials"),
            (
                "HTTP 404 model grok-missing does not exist secret-body",
                "model_not_available",
            ),
            ("HTTP 500 secret-body", "provider_unreachable"),
        ];
        for (detail, expected) in cases {
            let item =
                classified_provider_failure("ai", "AI provider", detail, "Check configuration.");
            assert_eq!(item.code, expected);
            assert_eq!(item.status, CheckStatus::Failed);
            assert!(!serde_json::to_string(&item)
                .unwrap()
                .contains("secret-body"));
        }
    }

    #[test]
    fn http_status_wins_over_conflicting_error_body_text() {
        let forbidden = classified_provider_failure(
            "ai",
            "AI provider",
            "provider error: AI returned HTTP 403 Forbidden: mentions rate limit 429",
            "Check credentials.",
        );
        assert_eq!(forbidden.code, "invalid_credentials");

        let server_error = classified_provider_failure(
            "ai",
            "AI provider",
            "provider error: AI returned HTTP 500 Internal Server Error: mentions 429",
            "Check provider.",
        );
        assert_eq!(server_error.code, "provider_unreachable");
    }

    #[test]
    fn live_value_is_reduced_to_safe_typed_check() {
        let live = serde_json::json!({
            "reachable": false,
            "detail": "Provider returned HTTP 403: echoed-token-canary"
        });
        let item = live_provider_check("tavily", "Tavily", Some(&live), "Set TAVILY_API_KEY.");
        assert_eq!(item.code, "invalid_credentials");
        assert!(!serde_json::to_string(&item)
            .unwrap()
            .contains("echoed-token-canary"));
    }

    #[test]
    fn optional_provider_degradation_keeps_core_ok() {
        let summary = config_summary(
            &Config::from_env_map(Vec::<(String, String)>::new()),
            "missing".to_string(),
            None,
        );
        let report = finish_report(summary, Vec::new(), true, true);
        assert!(report.ok);
        assert_eq!(report.status, DoctorStatus::Degraded);
    }

    #[test]
    fn missing_or_disabled_optional_provider_does_not_degrade_core() {
        for plan in [OptionalProbePlan::Missing, OptionalProbePlan::Disabled] {
            let (item, degraded) =
                optional_check_from_live("tavily", "Tavily", plan, None, "Set TAVILY_API_KEY.");
            assert_eq!(item.status, CheckStatus::Skipped);
            assert!(!degraded);
        }
    }

    #[test]
    fn whitespace_grok_key_does_not_steal_compatible_transport() {
        let config = Config::from_env_map([
            ("GROK_SEARCH_API_KEY", "   "),
            ("OPENAI_COMPATIBLE_API_URL", "https://compat.example/v1"),
            ("OPENAI_COMPATIBLE_API_KEY", "sk-fake"),
        ]);
        assert!(config.grok_api_key.is_none());
        assert_eq!(config.transport, Transport::ChatCompletions);
    }

    #[test]
    fn config_path_is_reported_but_auth_path_is_not() {
        let mut config = Config::from_env_map(Vec::<(String, String)>::new());
        config.grok_auth_file = Some(PathBuf::from("/private/auth/canary-auth.json"));
        let summary = config_summary(
            &config,
            "loaded".to_string(),
            Some(std::path::Path::new("/safe/config.toml")),
        );
        let text = serde_json::to_string(&summary).unwrap();
        assert!(text.contains("/safe/config.toml"));
        assert!(!text.contains("canary-auth.json"));
    }

    #[test]
    fn valid_url_keeps_only_non_secret_components() {
        assert_eq!(
            validated_base_url("https://api.example.com:8443/v1/"),
            Some("https://api.example.com:8443".to_string())
        );
        assert_eq!(
            validated_base_url("ftp://api.example.com/v1"),
            None,
            "non-http(s) schemes must be rejected"
        );
    }

    #[test]
    fn url_path_is_not_exposed_in_diagnostics() {
        let raw = "https://relay.example/tenant/path-token-canary/v1";
        assert_eq!(safe_base_url(raw), "https://relay.example");
        assert!(!safe_base_url(raw).contains("path-token-canary"));
    }

    #[test]
    fn ipv6_origin_remains_unambiguous() {
        assert_eq!(
            safe_base_url("http://[::1]:8080/private/v1"),
            "http://[::1]:8080"
        );
    }

    #[test]
    fn text_renderer_escapes_terminal_control_characters() {
        assert_eq!(terminal_safe("model\n\u{1b}[31m"), "model\\n\\u{1b}[31m");
        assert_eq!(terminal_safe("模型/配置"), "模型/配置");
    }
}
