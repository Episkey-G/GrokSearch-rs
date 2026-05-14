use serde::{Deserialize, Serialize};

use crate::model::source::Source;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchInput {
    pub query: String,
    pub platform: Option<String>,
    pub model: Option<String>,
    pub extra_sources: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchOutput {
    pub session_id: String,
    pub content: String,
    pub sources_count: usize,
    pub search_provider: String,
    pub fallback_used: bool,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetSourcesOutput {
    pub session_id: String,
    pub sources: Vec<Source>,
    pub sources_count: usize,
}
