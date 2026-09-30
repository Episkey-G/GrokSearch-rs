use crate::model::source::Source;

/// Drop sources with empty URLs and de-duplicate by URL, preserving the order
/// of first appearance. Shared by every response adapter so newer extraction
/// paths inherit the same invariant for free.
pub fn dedupe_sources(sources: &mut Vec<Source>) {
    let mut seen = std::collections::HashSet::new();
    sources.retain(|source| !source.url.trim().is_empty() && seen.insert(source.url.clone()));
}

/// Label and stable-sort `sources` so what the answer rests on leads: the
/// `cited` URLs in citation order (`evidence: "cited"`), then the `opened`
/// pages (`evidence: "opened"`), then everything else in its original order.
/// Raw search-hit lists otherwise bury them past the enrichment window and the
/// response budget's tail trim, both of which assume citations lead. Run after
/// `dedupe_sources` so the richer structured entry is the one that gets
/// ranked.
pub fn rank_used_first(sources: &mut [Source], cited: &[Source], opened: &[Source]) {
    let used: Vec<(&str, &'static str)> = cited
        .iter()
        .map(|source| (source.url.as_str(), "cited"))
        .chain(opened.iter().map(|source| (source.url.as_str(), "opened")))
        .collect();
    if used.is_empty() {
        return;
    }
    for source in sources.iter_mut() {
        if let Some(&(_, evidence)) = used.iter().find(|(url, _)| *url == source.url) {
            source.evidence = Some(evidence.into());
        }
    }
    sources.sort_by_cached_key(|source| {
        used.iter()
            .position(|(url, _)| *url == source.url)
            .unwrap_or(usize::MAX)
    });
}

/// Find inline citations of the form `[[n]](https://...)` and `[[n]](http://...)`
/// in the response text and push them as sources under `provider`. We avoid the
/// `regex` crate to keep the dependency footprint flat — a hand-rolled scanner is
/// sufficient for this fixed pattern.
///
/// Shared by every response adapter: OpenAI-compatible gateways (and proxied
/// Grok Responses endpoints) frequently serialize real search citations as inline
/// Markdown links in the answer text instead of structured citation fields. This
/// is a last-resort extraction path — run it after the structured paths so
/// `dedupe_sources` folds duplicates into the richer structured entries. Where
/// no positioned annotations exist, the URLs it finds are also what
/// `rank_used_first` moves to the front.
///
/// On any malformed match (missing `]]`, missing `(`, missing closing `)`,
/// non-numeric inside `[[...]]`), advance past the offending `[[` and keep
/// scanning so a single bad citation never wipes out later valid ones.
pub fn extract_inline_bracket_citations(
    content: &str,
    provider: &'static str,
    out: &mut Vec<Source>,
) {
    let mut offset = 0usize;
    while offset < content.len() {
        let remaining = &content[offset..];
        let Some(rel_idx) = remaining.find("[[") else {
            break;
        };
        let abs = offset + rel_idx;
        let after = &content[abs + 2..];
        let Some(close_brackets) = after.find("]]") else {
            // No `]]` anywhere ahead — nothing more to find. Bail out.
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
                                // Find the first `)` *that closes a well-formed URL*. URLs do not
                                // legitimately contain whitespace or another `[`, so treat those as
                                // bailout signals — otherwise a missing-`)` citation could swallow
                                // the rest of the document into a single bogus URL.
        let url_window = &after_brackets[url_start..];
        let mut bail: Option<usize> = None;
        for (i, ch) in url_window.char_indices() {
            if ch == ')' {
                bail = Some(i);
                break;
            }
            if ch == ' ' || ch == '\n' || ch == '\t' || ch == '[' || ch == '<' {
                break;
            }
        }
        let Some(close_paren) = bail else {
            // Malformed: no `)` ahead before whitespace / next bracket. Skip
            // past this `[[` and keep scanning; earlier code dropped every
            // later citation in the same response.
            offset = abs + 2;
            continue;
        };
        let url = &url_window[..close_paren];
        if url.starts_with("http://") || url.starts_with("https://") {
            out.push(Source::new(url, provider));
        }
        // advance past this whole match
        offset = (abs + 2) + close_brackets + 2 + 1 + close_paren + 1;
    }
}
