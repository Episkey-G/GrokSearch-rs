use serde::{Deserialize, Serialize};

use crate::model::source::Source;
use crate::sources::SourceType;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WebSearchInput {
    pub query: String,
    pub platform: Option<String>,
    pub model: Option<String>,
    pub extra_sources: Option<usize>,
    pub recency_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_domains: Vec<String>,
    pub include_content: Option<bool>,
    /// `"concise"` (answer + source metadata only) or `"detailed"` (inline
    /// content, subject to the response budget). When set, takes precedence
    /// over the legacy `include_content` flag.
    pub response_format: Option<String>,
    /// Per-call reasoning intensity override: `low` | `medium` | `high` |
    /// `xhigh` | `max`. When set, beats the server default
    /// (`GROK_SEARCH_REASONING_EFFORT` / `X-Grok-Reasoning-Effort`).
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchOutput {
    pub session_id: String,
    pub content: String,
    pub sources_count: usize,
    pub sources: Vec<Source>,
    pub search_provider: String,
    pub fallback_used: bool,
    pub fallback_reason: Option<String>,
    /// True when the response budget trimmed inline source content. The cache
    /// keeps the full text — recover via `get_sources` or `web_fetch`.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetSourcesOutput {
    pub session_id: String,
    pub sources: Vec<Source>,
    /// Number of sources in THIS page (`sources.len()`), not the cache total.
    pub sources_count: usize,
    /// Total sources cached for the session, across all pages.
    pub total_sources: usize,
    pub offset: usize,
    /// Offset of the next page; absent when this page reaches the end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    /// True when the response budget trimmed inline content within this page.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFetchOutput {
    pub url: String,
    pub content: String,
    pub original_length: usize,
    pub truncated: bool,
    /// Always present. `generic` on the fallback path (decision D-02);
    /// specialist values arrive in Phase 2.
    pub source_type: SourceType,
    /// Present only when a specialist was matched and then failed
    /// (decision D-01); omitted from JSON otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceType;

    #[test]
    fn web_fetch_output_includes_source_type_and_omits_none_fallback_reason() {
        let output = WebFetchOutput {
            url: "https://example.com".to_string(),
            content: "hi".to_string(),
            original_length: 2,
            truncated: false,
            source_type: SourceType::Generic,
            fallback_reason: None,
        };
        let value = serde_json::to_value(&output).unwrap();
        assert_eq!(value["source_type"], "generic");
        assert!(value.get("fallback_reason").is_none());
    }

    #[test]
    fn web_fetch_output_serializes_fallback_reason_when_present() {
        let output = WebFetchOutput {
            url: "https://example.com".to_string(),
            content: "hi".to_string(),
            original_length: 2,
            truncated: false,
            source_type: SourceType::Generic,
            fallback_reason: Some("github_issue empty render".to_string()),
        };
        let value = serde_json::to_value(&output).unwrap();
        assert_eq!(value["fallback_reason"], "github_issue empty render");
    }
}
