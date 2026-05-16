use crate::adapters::sources::dedupe_sources;
use crate::error::{GrokSearchError, Result};
use crate::model::search::SearchResponse;
use crate::model::source::Source;
use serde_json::Value;

pub fn parse_grok_responses(raw: &Value) -> Result<SearchResponse> {
    let mut text_parts = Vec::new();
    let mut sources = Vec::new();

    if let Some(output_text) = raw.get("output_text").and_then(Value::as_str) {
        push_nonempty(&mut text_parts, output_text);
    }

    if let Some(output) = raw.get("output").and_then(Value::as_array) {
        for item in output {
            collect_output_item(item, &mut text_parts, &mut sources);
        }
    }

    if let Some(citations) = raw.get("citations") {
        collect_sources_from_value(citations, &mut sources);
    }

    dedupe_sources(&mut sources);

    let content = text_parts.join("\n").trim().to_string();
    if content.is_empty() && sources.is_empty() {
        return Err(GrokSearchError::Parse(
            "Grok Responses payload did not contain text or sources".to_string(),
        ));
    }

    Ok(SearchResponse { content, sources })
}

fn collect_output_item(item: &Value, text_parts: &mut Vec<String>, sources: &mut Vec<Source>) {
    if item.get("type").and_then(Value::as_str) == Some("web_search_call") {
        if let Some(action_sources) = item.get("action").and_then(|action| action.get("sources")) {
            collect_sources_from_value(action_sources, sources);
        }
    }

    if let Some(content) = item.get("content").and_then(Value::as_array) {
        for block in content {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                push_nonempty(text_parts, text);
            }
            if let Some(annotations) = block.get("annotations") {
                collect_sources_from_value(annotations, sources);
            }
            if let Some(citations) = block.get("citations") {
                collect_sources_from_value(citations, sources);
            }
        }
    }
}

fn collect_sources_from_value(value: &Value, sources: &mut Vec<Source>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_one_source(item, sources);
            }
        }
        Value::Object(_) => collect_one_source(value, sources),
        _ => {}
    }
}

fn collect_one_source(item: &Value, sources: &mut Vec<Source>) {
    if let Some(url) = item.as_str() {
        sources.push(Source::new(url, "grok_responses"));
        return;
    }

    let Some(url) = item
        .get("url")
        .or_else(|| item.get("uri"))
        .and_then(Value::as_str)
    else {
        return;
    };

    let mut source = Source::new(url, "grok_responses");
    if let Some(title) = item.get("title").and_then(Value::as_str) {
        source = source.with_title(title);
    }
    if let Some(description) = item
        .get("description")
        .or_else(|| item.get("snippet"))
        .or_else(|| item.get("content"))
        .and_then(Value::as_str)
    {
        source = source.with_description(description);
    }
    if let Some(published_date) = item
        .get("published_date")
        .or_else(|| item.get("publishedDate"))
        .and_then(Value::as_str)
    {
        source = source.with_published_date(published_date);
    }
    sources.push(source);
}

fn push_nonempty(text_parts: &mut Vec<String>, text: &str) {
    let trimmed = text.trim();
    if !trimmed.is_empty() && !text_parts.iter().any(|item| item == trimmed) {
        text_parts.push(trimmed.to_string());
    }
}

