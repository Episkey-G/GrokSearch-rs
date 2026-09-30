use crate::adapters::sources::{dedupe_sources, extract_inline_bracket_citations, rank_used_first};
use crate::error::{GrokSearchError, Result};
use crate::model::search::SearchResponse;
use crate::model::source::Source;
use serde_json::Value;

pub fn parse_grok_responses(raw: &Value) -> Result<SearchResponse> {
    let mut text_parts = Vec::new();
    let mut sources = Vec::new();
    let mut cited = Vec::new();
    let mut opened = Vec::new();

    if let Some(output_text) = raw.get("output_text").and_then(Value::as_str) {
        push_nonempty(&mut text_parts, output_text);
    }

    if let Some(output) = raw.get("output").and_then(Value::as_array) {
        for item in output {
            collect_output_item(item, &mut text_parts, &mut sources, &mut cited, &mut opened);
        }
    }

    if let Some(citations) = raw.get("citations") {
        collect_sources_from_value(citations, &mut sources);
    }

    let content = text_parts.join("\n").trim().to_string();

    // Last-resort path: proxied / OpenAI-compatible Grok gateways often inline
    // real search citations as `[[n]](url)` Markdown in the answer text instead
    // of the structured fields above. Harvest those (and the opened pages)
    // after the structured paths so dedupe folds duplicates into the richer
    // structured entries.
    let mut inline = Vec::new();
    extract_inline_bracket_citations(&content, "grok_responses", &mut inline);
    sources.extend(inline.iter().cloned());
    sources.extend(opened.iter().cloned());

    dedupe_sources(&mut sources);
    // Label and rank what the answer rests on ahead of the raw search hits: the
    // citations (api.x.ai's positioned annotations carry the exact URLs;
    // gateways without them only have the inline links), then the pages Grok
    // opened.
    let cited = if cited.is_empty() { inline } else { cited };
    rank_used_first(&mut sources, &cited, &opened);

    if content.is_empty() && sources.is_empty() {
        return Err(GrokSearchError::Parse(
            "Grok Responses payload did not contain text or sources".to_string(),
        ));
    }

    Ok(SearchResponse { content, sources })
}

fn collect_output_item(
    item: &Value,
    text_parts: &mut Vec<String>,
    sources: &mut Vec<Source>,
    cited: &mut Vec<Source>,
    opened: &mut Vec<Source>,
) {
    if item.get("type").and_then(Value::as_str) == Some("web_search_call") {
        let action = item.get("action");
        if let Some(action_sources) = action.and_then(|action| action.get("sources")) {
            collect_sources_from_value(action_sources, sources);
        }
        // `open_page` / `find_in_page` actions carry no `sources`, only the
        // page Grok read, which no search hit may list. A failed open read
        // nothing, so it is not evidence.
        let completed = matches!(
            item.get("status").and_then(Value::as_str),
            None | Some("completed")
        );
        if let Some(url) = action
            .and_then(|action| action.get("url"))
            .and_then(Value::as_str)
            .filter(|_| completed)
        {
            opened.push(Source::new(url, "grok_responses"));
        }
    }

    if let Some(content) = item.get("content").and_then(Value::as_array) {
        for block in content {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                push_nonempty(text_parts, text);
            }
            if let Some(annotations) = block.get("annotations") {
                collect_sources_from_value(annotations, sources);
                // A `url_citation` whose span covers text is one the answer
                // cites; zero-width (0..0) ones only list what the search
                // encountered.
                for annotation in annotations.as_array().into_iter().flatten() {
                    let index = |key| annotation.get(key).and_then(Value::as_u64).unwrap_or(0);
                    if index("end_index") > index("start_index") {
                        collect_one_source(annotation, cited);
                    }
                }
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
    // Junk guard (issue #21): live api.x.ai annotations carry the citation
    // index as the title ("1", "2"). Dropping them here keeps the field None
    // so enrichment-time backfill is allowed to fill in a real title later.
    if let Some(title) = item
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !crate::model::source::is_junk_title(title))
    {
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
