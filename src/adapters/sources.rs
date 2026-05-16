use crate::model::source::Source;

/// Drop sources with empty URLs and de-duplicate by URL, preserving the order
/// of first appearance. Shared by every response adapter so newer extraction
/// paths inherit the same invariant for free.
pub fn dedupe_sources(sources: &mut Vec<Source>) {
    let mut seen = std::collections::HashSet::new();
    sources.retain(|source| !source.url.trim().is_empty() && seen.insert(source.url.clone()));
}
