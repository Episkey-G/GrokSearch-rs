use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    pub provider: Cow<'static, str>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub published_date: Option<String>,
}

impl Source {
    pub fn new(url: impl Into<String>, provider: impl Into<Cow<'static, str>>) -> Self {
        Self {
            url: url.into(),
            provider: provider.into(),
            title: None,
            description: None,
            published_date: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_published_date(mut self, published_date: impl Into<String>) -> Self {
        self.published_date = Some(published_date.into());
        self
    }
}

pub fn merge_sources(primary: Vec<Source>, secondary: Vec<Source>) -> Vec<Source> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for source in primary.into_iter().chain(secondary) {
        if source.url.trim().is_empty() {
            continue;
        }
        if seen.insert(source.url.clone()) {
            merged.push(source);
        }
    }
    merged
}
