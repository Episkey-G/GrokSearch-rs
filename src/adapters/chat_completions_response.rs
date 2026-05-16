use crate::adapters::sources::dedupe_sources;
use crate::error::{GrokSearchError, Result};
use crate::model::search::SearchResponse;
use crate::model::source::Source;
use serde_json::Value;

const PROVIDER_LABEL: &str = "openai_compatible";

/// Parse an OpenAI-style chat-completions response. Source extraction runs
/// four paths in priority order, then de-dupes by URL preserving order:
///
///   1. `choices[0].message.annotations[].url_citation` — OpenAI standard
///   2. `choices[0].message.citations` and top-level `citations` —
///      Perplexity / various OpenAI-compat gateways
///   3. top-level `search_sources` — marybrown-style auto-search gateways
///   4. inline `[[n]](url)` markers in the content text — last-resort fallback
pub fn parse_chat_completions(raw: &Value) -> Result<SearchResponse> {
    let content = raw
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    let mut sources: Vec<Source> = Vec::new();

    // path 1: message.annotations
    if let Some(anns) = raw.pointer("/choices/0/message/annotations") {
        collect_sources_from_value(anns, &mut sources);
    }

    // path 2a: message.citations
    if let Some(cits) = raw.pointer("/choices/0/message/citations") {
        collect_sources_from_value(cits, &mut sources);
    }
    // path 2b: top-level citations
    if let Some(cits) = raw.get("citations") {
        collect_sources_from_value(cits, &mut sources);
    }

    // path 3: top-level search_sources
    if let Some(ss) = raw.get("search_sources") {
        collect_sources_from_value(ss, &mut sources);
    }

    // path 4: inline [[n]](url) in the content
    extract_inline_bracket_citations(&content, &mut sources);

    dedupe_sources(&mut sources);

    if content.is_empty() && sources.is_empty() {
        return Err(GrokSearchError::Parse(
            "chat/completions response is empty and has no sources".to_string(),
        ));
    }

    Ok(SearchResponse { content, sources })
}

fn collect_sources_from_value(value: &Value, out: &mut Vec<Source>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_one(item, out);
            }
        }
        Value::Object(_) => collect_one(value, out),
        _ => {}
    }
}

fn collect_one(item: &Value, out: &mut Vec<Source>) {
    if let Some(url) = item.as_str() {
        if url.starts_with("http://") || url.starts_with("https://") {
            out.push(Source::new(url, PROVIDER_LABEL));
        }
        return;
    }
    let Some(url) = item
        .get("url")
        .or_else(|| item.get("uri"))
        .or_else(|| item.get("href"))
        .or_else(|| item.get("link"))
        .and_then(Value::as_str)
    else {
        return;
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let mut source = Source::new(url, PROVIDER_LABEL);
    if let Some(t) = item
        .get("title")
        .or_else(|| item.get("name"))
        .and_then(Value::as_str)
    {
        source = source.with_title(t);
    }
    if let Some(d) = item
        .get("description")
        .or_else(|| item.get("snippet"))
        .or_else(|| item.get("content"))
        .and_then(Value::as_str)
    {
        source = source.with_description(d);
    }
    if let Some(p) = item
        .get("published_date")
        .or_else(|| item.get("publishedDate"))
        .and_then(Value::as_str)
    {
        source = source.with_published_date(p);
    }
    out.push(source);
}

/// Find inline citations of the form `[[n]](https://...)` and `[[n]](http://...)`
/// in the response text. We avoid the `regex` crate to keep the dependency
/// footprint flat — a hand-rolled scanner is sufficient for this fixed pattern.
fn extract_inline_bracket_citations(content: &str, out: &mut Vec<Source>) {
    let mut offset = 0usize;
    while offset < content.len() {
        let remaining = &content[offset..];
        let Some(rel_idx) = remaining.find("[[") else {
            break;
        };
        let abs = offset + rel_idx;
        let after = &content[abs + 2..];
        let Some(close_brackets) = after.find("]]") else {
            break;
        };
        let between = &after[..close_brackets];
        if between.is_empty() || !between.chars().all(|c| c.is_ascii_digit()) {
            offset = abs + 2;
            continue;
        }
        let after_brackets = &after[close_brackets + 2..];
        if !after_brackets.starts_with('(') {
            offset = abs + 2;
            continue;
        }
        let url_start = 1usize; // skip the '('
        let Some(close_paren) = after_brackets[url_start..].find(')') else {
            break;
        };
        let url = &after_brackets[url_start..url_start + close_paren];
        if url.starts_with("http://") || url.starts_with("https://") {
            out.push(Source::new(url, PROVIDER_LABEL));
        }
        // advance past this whole match
        offset = (abs + 2) + close_brackets + 2 + 1 + close_paren + 1;
    }
}
