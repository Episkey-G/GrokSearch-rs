use async_trait::async_trait;
use reqwest::header::USER_AGENT;
use reqwest::Client;
use url::Url;

use crate::error::{GrokSearchError, Result};
use crate::sources::{get_json, SourceCaps, SourceExtractor, SourceType};

const UA: &str = "grok-search-rs/0.1 (https://github.com/Episkey-G/GrokSearch-rs)";

/// StackExchange API filter that adds `question.body_markdown` /
/// `answer.body_markdown` on top of the `withbody` base. Without it the API only
/// returns the rendered HTML `body` field, so `field_str(.., "body_markdown",
/// "body")` always fell through to HTML — defeating the Markdown extraction.
///
/// Filter strings are deterministic (same `base` + `include` set always yields
/// this exact value) and do not expire. Regenerate with:
///   GET https://api.stackexchange.com/2.3/filters/create
///       ?base=withbody&include=question.body_markdown;answer.body_markdown&unsafe=false
const SE_FILTER: &str = "!X-cWn5YrCQCchzB5B4*yqi6eO0BYWbSmsTE.VZm";

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SeComment {
    pub author: String,
    pub body: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SeAnswer {
    pub is_accepted: bool,
    pub score: i64,
    pub author: String,
    pub body: String,
    pub comments: Vec<SeComment>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SeRaw {
    pub title: String,
    pub body: String,
    pub site: String,
    pub answers: Vec<SeAnswer>,
}

pub struct StackExchangeExtractor;

/// api_site_parameter for a non-meta host.
fn base_site_param(host: &str) -> String {
    match host {
        "stackoverflow.com" => "stackoverflow".to_string(),
        "serverfault.com" => "serverfault".to_string(),
        "superuser.com" => "superuser".to_string(),
        "askubuntu.com" => "askubuntu".to_string(),
        // MathOverflow is the exception: its api_site_parameter is the full
        // domain, not a stripped subdomain.
        "mathoverflow.net" => "mathoverflow.net".to_string(),
        other => other
            .strip_suffix(".stackexchange.com")
            .unwrap_or(other)
            .to_string(),
    }
}

fn site_from_host(host: &str) -> String {
    // Meta hosts (central `meta.stackexchange.com` and per-site
    // `meta.stackoverflow.com`, `meta.serverfault.com`, …) map to the
    // `meta.<base>` api_site_parameter — naive suffix stripping would yield a
    // bare `meta` and break the API call.
    if let Some(base) = host.strip_prefix("meta.") {
        if base == "stackexchange.com" {
            return "meta.stackexchange".to_string();
        }
        return format!("meta.{}", base_site_param(base));
    }
    base_site_param(host)
}

fn is_se_host(host: &str) -> bool {
    matches!(
        host,
        "stackoverflow.com"
            | "serverfault.com"
            | "superuser.com"
            | "askubuntu.com"
            | "mathoverflow.net"
            | "meta.stackoverflow.com"
            | "meta.serverfault.com"
            | "meta.superuser.com"
            | "meta.askubuntu.com"
            | "meta.mathoverflow.net"
    ) || host.ends_with(".stackexchange.com")
}

/// StackExchange answers page size. Defaults to 30/page; request
/// `max_answers + 1` (capped at the API's 100) so `render` sees every answer
/// within the cap and can emit its "more answers" marker. Mirrors the GitHub
/// comments paging approach.
fn answers_pagesize(max_answers: usize) -> usize {
    max_answers.saturating_add(1).min(100)
}

/// Question endpoint URL. Carries [`SE_FILTER`] so the response includes
/// `body_markdown` (not just the HTML `body`). Pure (no I/O) so it is unit-tested.
fn question_url(id: &str, site: &str) -> String {
    format!("https://api.stackexchange.com/2.3/questions/{id}?site={site}&filter={SE_FILTER}")
}

/// Answers endpoint URL: vote-sorted, [`SE_FILTER`] for `body_markdown`, paged to
/// `answers_pagesize`. Pure (no I/O) so it is unit-tested.
fn answers_url(id: &str, site: &str, max_answers: usize) -> String {
    format!(
        "https://api.stackexchange.com/2.3/questions/{id}/answers?site={site}&filter={SE_FILTER}&order=desc&sort=votes&pagesize={}",
        answers_pagesize(max_answers)
    )
}

/// Decode the small set of HTML entities StackExchange emits inside
/// `body_markdown`, titles, and display names (`&lt; &gt; &amp; &quot; &apos;`
/// plus numeric `&#NN;` / `&#xHH;`). SE stores Markdown source with these
/// encoded, so `c &lt; arraySize` and `Poincar&#233;` would otherwise leak into
/// the rendered output. Unknown entities (e.g. `&nbsp;`) and bare `&` are left
/// verbatim so real text is never corrupted. Pure — unit-tested offline.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let decoded = tail.find(';').and_then(|semi| {
            let ent = &tail[1..semi];
            let c = match ent {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => ent.strip_prefix('#').and_then(|num| {
                    match num.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => num.parse::<u32>().ok(),
                    }
                    .and_then(char::from_u32)
                }),
            };
            c.map(|c| (c, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            // Not a recognized entity: keep the '&' literal, advance past it.
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn field_str(v: &serde_json::Value, primary: &str, fallback: &str) -> String {
    let raw = v
        .get(primary)
        .or_else(|| v.get(fallback))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    decode_entities(raw)
}

fn owner_name(v: &serde_json::Value) -> String {
    let raw = v
        .get("owner")
        .and_then(|o| o.get("display_name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    decode_entities(raw)
}

/// Map an `/answers` (or embedded `answers`) JSON payload into `SeAnswer`s.
/// Tolerant of missing fields so a partial response still yields usable bodies.
fn parse_answers(json: &serde_json::Value) -> Vec<SeAnswer> {
    json.get("items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|a| SeAnswer {
                    is_accepted: a
                        .get("is_accepted")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    score: a.get("score").and_then(|v| v.as_i64()).unwrap_or(0),
                    author: owner_name(a),
                    body: field_str(a, "body_markdown", "body"),
                    comments: a
                        .get("comments")
                        .and_then(|v| v.as_array())
                        .map(|carr| {
                            carr.iter()
                                .map(|c| SeComment {
                                    author: owner_name(c),
                                    body: field_str(c, "body_markdown", "body"),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) async fn fetch(client: &Client, url: &Url, max_answers: usize) -> Result<SeRaw> {
    let host = url.host_str().unwrap_or("");
    let site = site_from_host(host);
    let segs: Vec<&str> = url
        .path_segments()
        .map(|it| it.filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let id = segs.get(1).copied().unwrap_or_default();
    let headers = [(USER_AGENT, UA)];

    // The question (`/questions/{id}`) returns the QUESTION body but never the
    // answers array; answers come from the dedicated, vote-sorted endpoint. The
    // answers URL depends only on `id`/`site` (from the URL), not on the question
    // response, so the two GETs have no data dependency and run concurrently —
    // halving the round-trip latency (mirrors the GitHub PR `tokio::join!` path).
    //
    // The question is mandatory (`?`); the answers call is best-effort: a
    // failed/rate-limited answers fetch still returns the question rather than
    // failing the whole specialist. NOTE: per-answer comments still need a custom
    // SE filter (via /filters/create) and remain out of scope; the renderer
    // degrades gracefully when comment lists are empty. Anonymous calls are
    // rate-limited (~300/day); a future key could lift that.
    let q_url = question_url(id, &site);
    let a_url = answers_url(id, &site, max_answers);
    let (q_res, a_res) = tokio::join!(
        get_json(client, &q_url, &headers, "stackexchange"),
        get_json(client, &a_url, &headers, "stackexchange"),
    );

    let q_json = q_res?;
    let item = q_json
        .get("items")
        .and_then(|i| i.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| GrokSearchError::Provider("stackexchange: empty items".into()))?;

    let answers = match a_res {
        Ok(a_json) => parse_answers(&a_json),
        Err(_) => Vec::new(),
    };

    Ok(SeRaw {
        title: decode_entities(
            item.get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        ),
        body: field_str(item, "body_markdown", "body"),
        site,
        answers,
    })
}

pub fn render(raw: &SeRaw, caps: &SourceCaps) -> String {
    let mut answers: Vec<&SeAnswer> = raw.answers.iter().collect();
    answers.sort_by(|a, b| {
        b.is_accepted
            .cmp(&a.is_accepted)
            .then(b.score.cmp(&a.score))
    });
    let total = answers.len();

    let mut out = format!("# {}\n\n{}\n\n---\n\n", raw.title, raw.body);
    for ans in answers.iter().take(caps.max_answers) {
        if ans.is_accepted {
            out.push_str(&format!(
                "## ★ 采纳答案 (↑{})\n\n{}\n\n",
                ans.score, ans.body
            ));
            for c in ans.comments.iter().take(caps.max_comments) {
                out.push_str(&format!("> **{}:** {}\n", c.author, c.body));
            }
            if ans.comments.len() > caps.max_comments {
                let extra = ans.comments.len() - caps.max_comments;
                out.push_str(&format!("_还有 {extra} 条评论_\n"));
            }
            out.push('\n');
        } else {
            out.push_str(&format!(
                "## 答案 by {} (↑{})\n\n{}\n\n",
                ans.author, ans.score, ans.body
            ));
        }
    }
    if total > caps.max_answers {
        let extra = total - caps.max_answers;
        out.push_str(&format!("_还有 {extra} 条_\n"));
    }
    out
}

#[async_trait]
impl SourceExtractor for StackExchangeExtractor {
    fn matches(&self, url: &Url) -> bool {
        let host = url.host_str().unwrap_or("");
        if !is_se_host(host) {
            return false;
        }
        let segs: Vec<&str> = match url.path_segments() {
            Some(it) => it.filter(|s| !s.is_empty()).collect(),
            None => return false,
        };
        segs.len() >= 2 && segs[0] == "questions" && segs[1].parse::<u64>().is_ok()
    }
    fn kind(&self) -> SourceType {
        SourceType::Stackexchange
    }
    async fn fetch_render(&self, client: &Client, url: &Url, caps: &SourceCaps) -> Result<String> {
        let raw = fetch(client, url, caps.max_answers).await?;
        Ok(render(&raw, caps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_from_host_preserves_meta_stackexchange() {
        // Meta Stack Exchange's api_site_parameter is "meta.stackexchange",
        // not "meta" — stripping the suffix would break the specialist path.
        assert_eq!(
            site_from_host("meta.stackexchange.com"),
            "meta.stackexchange"
        );
    }

    #[test]
    fn site_from_host_strips_regular_stackexchange_subdomain() {
        assert_eq!(site_from_host("math.stackexchange.com"), "math");
        assert_eq!(site_from_host("stackoverflow.com"), "stackoverflow");
    }

    #[test]
    fn site_from_host_keeps_mathoverflow_full_domain() {
        // MathOverflow's api_site_parameter is the full domain "mathoverflow.net".
        assert_eq!(site_from_host("mathoverflow.net"), "mathoverflow.net");
    }

    #[test]
    fn site_from_host_maps_per_site_meta_hosts() {
        // Per-site metas use api_site_parameter "meta.<base>".
        assert_eq!(
            site_from_host("meta.stackoverflow.com"),
            "meta.stackoverflow"
        );
        assert_eq!(site_from_host("meta.serverfault.com"), "meta.serverfault");
        assert_eq!(site_from_host("meta.askubuntu.com"), "meta.askubuntu");
    }

    #[test]
    fn is_se_host_accepts_per_site_meta_hosts() {
        assert!(is_se_host("meta.stackoverflow.com"));
        assert!(is_se_host("meta.superuser.com"));
        assert!(is_se_host("meta.stackexchange.com"));
        assert!(is_se_host("meta.mathoverflow.net"));
        assert!(!is_se_host("example.com"));
    }

    #[test]
    fn site_from_host_maps_meta_mathoverflow() {
        assert_eq!(
            site_from_host("meta.mathoverflow.net"),
            "meta.mathoverflow.net"
        );
    }

    #[test]
    fn answers_pagesize_requests_one_more_than_cap_capped_at_100() {
        assert_eq!(answers_pagesize(5), 6);
        assert_eq!(answers_pagesize(250), 100);
    }

    // Regression: SE web_fetch rendered raw HTML instead of Markdown
    // Found by /qa on 2026-06-04
    // Report: .gstack/qa-reports/qa-report-stackexchange-2026-06-04.md
    //
    // Root cause was `filter=withbody`, which only returns the HTML `body`
    // field, so the `body_markdown`-preferring parser always fell through to
    // HTML. Both endpoint URLs must carry SE_FILTER (which adds body_markdown)
    // and must NOT request the bare `withbody` filter.
    #[test]
    fn question_url_requests_markdown_filter_not_withbody() {
        let u = question_url("11227809", "stackoverflow");
        assert!(u.contains(&format!("filter={SE_FILTER}")), "got: {u}");
        assert!(!u.contains("filter=withbody"), "still HTML-only: {u}");
    }

    #[test]
    fn answers_url_requests_markdown_filter_and_vote_sort() {
        let u = answers_url("11227809", "stackoverflow", 5);
        assert!(u.contains(&format!("filter={SE_FILTER}")), "got: {u}");
        assert!(!u.contains("filter=withbody"), "still HTML-only: {u}");
        assert!(u.contains("sort=votes"), "got: {u}");
        assert!(u.ends_with("pagesize=6"), "got: {u}");
    }

    #[test]
    fn field_str_prefers_body_markdown_over_html_body() {
        let v = serde_json::json!({ "body_markdown": "*md*", "body": "<p>html</p>" });
        assert_eq!(field_str(&v, "body_markdown", "body"), "*md*");
    }

    // ISSUE-002: SE body_markdown is entity-encoded; field_str must decode it so
    // code blocks and prose render correctly.
    #[test]
    fn field_str_decodes_html_entities_in_body_markdown() {
        let v = serde_json::json!({ "body_markdown": "for (c = 0; c &lt; n; ++c) &quot;x&#39;y&quot;" });
        assert_eq!(
            field_str(&v, "body_markdown", "body"),
            "for (c = 0; c < n; ++c) \"x'y\""
        );
    }

    #[test]
    fn decode_entities_handles_named_decimal_and_hex() {
        assert_eq!(decode_entities("a &lt; b &gt; c &amp; d"), "a < b > c & d");
        assert_eq!(decode_entities("Poincar&#233;&#39;s"), "Poincaré's");
        assert_eq!(decode_entities("Poincar&#xE9;"), "Poincaré");
    }

    #[test]
    fn decode_entities_leaves_unknown_and_bare_ampersand_intact() {
        // Unknown named entity and a bare ampersand must survive unchanged so
        // real text (e.g. "R&D", "&nbsp;") is never corrupted.
        assert_eq!(decode_entities("R&D and Q&A"), "R&D and Q&A");
        assert_eq!(decode_entities("a &nbsp; b"), "a &nbsp; b");
        assert_eq!(decode_entities("no entities here"), "no entities here");
    }
}
