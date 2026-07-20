use crate::error::{GrokSearchError, Result};
use crate::model::tool::WebSearchInput;
use crate::service::SearchService;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run_stdio(service: SearchService) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                let response = error_response(Value::Null, -32700, format!("parse error: {err}"));
                stdout.write_all(response.to_string().as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                continue;
            }
        };

        if let Some(response) = handle_message(&service, request).await {
            stdout.write_all(response.to_string().as_bytes()).await?;
            stdout.write_all(b"\n").await?;
        }
    }

    Ok(())
}

/// Protocol revisions this server speaks, newest first. `initialize` echoes the
/// client's requested version when it is one of these — so existing stdio
/// clients that still request "2024-11-05" keep getting "2024-11-05" — and
/// otherwise declares [`LATEST_PROTOCOL_VERSION`].
pub(crate) const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Latest revision we support; declared when the client requests nothing or an
/// unsupported version.
pub(crate) const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// Pick the protocol revision to declare in the `initialize` result: the
/// client's requested revision when supported, else our latest.
pub(crate) fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    match requested {
        Some(req) => SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .find(|version| **version == req)
            .copied()
            .unwrap_or(LATEST_PROTOCOL_VERSION),
        None => LATEST_PROTOCOL_VERSION,
    }
}

pub(crate) async fn handle_message(service: &SearchService, request: Value) -> Option<Value> {
    request.get("id")?;
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    Some(
        handle_request(service, request)
            .await
            .unwrap_or_else(|err| {
                let code = err.code() as i64;
                error_response(id, code, err.to_string())
            }),
    )
}

async fn handle_request(service: &SearchService, request: Value) -> Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| GrokSearchError::InvalidParams("missing method".to_string()))?;

    match method {
        "initialize" => {
            // Negotiate: echo the client's requested revision when we speak it
            // (keeps existing stdio clients that still ask for "2024-11-05"
            // working), otherwise declare our latest supported revision.
            let requested = request
                .get("params")
                .and_then(|params| params.get("protocolVersion"))
                .and_then(Value::as_str);
            Ok(success_response(
                id,
                json!({
                    "protocolVersion": negotiate_protocol_version(requested),
                    "serverInfo": {
                        "name": "grok-search-rs",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": {
                        "tools": {}
                    }
                }),
            ))
        }
        "ping" => Ok(success_response(id, json!({}))),
        "tools/list" => Ok(success_response(id, tools_list())),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| GrokSearchError::InvalidParams("missing tool name".to_string()))?;
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = call_tool(service, name, args).await?;
            Ok(success_response(
                id,
                json!({
                    "content": [
                        {
                            "type": "text",
                            "text": result.to_string()
                        }
                    ],
                    "structuredContent": result
                }),
            ))
        }
        _ => Err(GrokSearchError::NotFound(format!(
            "unsupported method: {method}"
        ))),
    }
}

async fn call_tool(service: &SearchService, name: &str, args: Value) -> Result<Value> {
    match name {
        "doctor" => Ok(service.doctor().await),
        "web_search" => {
            let query = args.get("query").and_then(Value::as_str).ok_or_else(|| {
                GrokSearchError::InvalidParams("web_search.query is required".into())
            })?;
            let input = WebSearchInput {
                query: query.to_string(),
                platform: args
                    .get("platform")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                model: args
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                extra_sources: args
                    .get("extra_sources")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize),
                recency_days: args
                    .get("recency_days")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32)
                    .filter(|value| *value > 0),
                include_domains: parse_string_array(args.get("include_domains")),
                exclude_domains: parse_string_array(args.get("exclude_domains")),
                include_content: args.get("include_content").and_then(Value::as_bool),
                response_format: args
                    .get("response_format")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            let output = service.web_search(input).await?;
            Ok(serde_json::to_value(output)
                .map_err(|err| GrokSearchError::Parse(format!("serialize output: {err}")))?)
        }
        "get_sources" => {
            let session_id = args
                .get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    GrokSearchError::InvalidParams("get_sources.session_id is required".into())
                })?;
            let offset = args
                .get("offset")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(0);
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .filter(|value| *value > 0);
            let output = service.get_sources(session_id, offset, limit).await?;
            Ok(serde_json::to_value(output)
                .map_err(|err| GrokSearchError::Parse(format!("serialize sources: {err}")))?)
        }
        "web_fetch" => {
            let url = args.get("url").and_then(Value::as_str).ok_or_else(|| {
                GrokSearchError::InvalidParams("web_fetch.url is required".into())
            })?;
            let max_chars = args
                .get("max_chars")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .filter(|value| *value > 0);
            let output = service.web_fetch(url, max_chars).await?;
            Ok(serde_json::to_value(output)
                .map_err(|err| GrokSearchError::Parse(format!("serialize fetch: {err}")))?)
        }
        "web_map" => {
            let url = args
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| GrokSearchError::InvalidParams("web_map.url is required".into()))?;
            let max_results = args
                .get("max_results")
                .and_then(Value::as_u64)
                .unwrap_or(10) as usize;
            let sources = service.web_map(url, max_results).await?;
            let mapped_sources: Vec<Value> = sources
                .iter()
                .map(|source| json!({ "url": &source.url, "provider": &source.provider }))
                .collect();
            Ok(
                json!({ "url": url, "sources_count": mapped_sources.len(), "sources": mapped_sources }),
            )
        }
        _ => Err(GrokSearchError::NotFound(format!("unknown tool: {name}"))),
    }
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "web_search",
                "description": "Use for discovery — when you don't have a specific URL and need to find information, debug an error, research a topic, or track down an issue or news item. Returns an AI-synthesised answer plus a source list. By default the first few sources carry inline content (max_inline_sources, default 5); the rest are metadata-only — drill into any of them with web_fetch(url). The whole response is capped by a character budget; when truncated=true, trimmed sources carry a note telling you how to recover the full text via web_fetch or get_sources. Pass response_format=\"concise\" for answer + source metadata only. If you already know the exact page URL, use web_fetch instead.",
                "inputSchema": {
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": { "type": "string" },
                        "platform": { "type": "string" },
                        "model": { "type": "string" },
                        "extra_sources": {
                            "type": "integer",
                            "minimum": 0,
                            "description": "Optional supplemental source count. Tavily is primary; Firecrawl is fallback. If omitted, GROK_SEARCH_EXTRA_SOURCES is used."
                        },
                        "recency_days": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Restrict supplemental results to sources published within the last N days. Forwarded to Tavily as days+topic=news; also hinted to Grok prompt."
                        },
                        "include_domains": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Only return supplemental results from these domains. Tavily honors strictly; Grok receives as soft preference."
                        },
                        "exclude_domains": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Suppress supplemental results from these domains. Tavily honors strictly; Grok receives as soft instruction."
                        },
                        "include_content": {
                            "type": "boolean",
                            "default": true,
                            "description": "Inline source content via the resolve_content pipeline. Default true. Pass false to get summary + source-list only (legacy behavior, no content field in sources). Superseded by response_format when both are set."
                        },
                        "response_format": {
                            "type": "string",
                            "enum": ["concise", "detailed"],
                            "description": "concise = synthesized answer + source metadata only (smallest payload); detailed = inline source content, subject to the response budget. Takes precedence over include_content."
                        }
                    }
                }
            },
            {
                "name": "get_sources",
                "description": "Return cached sources from a previous web_search call by session_id. Use to re-examine sources already retrieved without issuing a new search — it reuses the prior session and runs no new search or fetch. Paginate with offset/limit: the response reports total_sources and, when more pages remain, next_offset to pass as the next offset.",
                "inputSchema": {
                    "type": "object",
                    "required": ["session_id"],
                    "properties": {
                        "session_id": { "type": "string" },
                        "offset": {
                            "type": "integer",
                            "minimum": 0,
                            "default": 0,
                            "description": "Index of the first source to return. Use next_offset from the previous page to continue."
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Max sources in this page. Omit to return all remaining sources (still subject to the response budget)."
                        }
                    }
                }
            },
            {
                "name": "web_fetch",
                "description": "Use when you already have a specific URL and want to read a single page in depth. GitHub issue/PR, StackOverflow (StackExchange), arXiv, and Wikipedia URLs are automatically parsed into structured, de-noised Markdown ready to feed an LLM; all other pages fall back to generic extraction. Returns {url, content, original_length, truncated, source_type, fallback_reason?}. If you don't have a URL yet and need to discover sources, use web_search instead.",
                "inputSchema": {
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": { "type": "string" },
                        "max_chars": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Optional character cap on returned content. Falls back to GROK_SEARCH_FETCH_MAX_CHARS, otherwise unlimited."
                        }
                    }
                }
            },
            {
                "name": "web_map",
                "description": "Map/discover URLs through Tavily Map.",
                "inputSchema": {
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": { "type": "string" },
                        "max_results": { "type": "integer", "minimum": 1 }
                    }
                }
            },
            {
                "name": "doctor",
                "description": "Diagnostic probe: live connectivity check for Grok, Tavily, and Firecrawl backends, plus masked configuration. Use to verify the server is wired up and reachable.",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}

fn parse_string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

pub(crate) fn error_response(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn initialized_notification_does_not_emit_response() {
        let service = SearchService::fake_with_sources();
        let response = handle_message(
            &service,
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }),
        )
        .await;

        assert_eq!(response, None);
    }

    #[test]
    fn negotiate_protocol_version_prefers_supported_client_version() {
        // Backward compatibility: an old client asking for 2024-11-05 is echoed.
        assert_eq!(negotiate_protocol_version(Some("2024-11-05")), "2024-11-05");
        assert_eq!(negotiate_protocol_version(Some("2025-06-18")), "2025-06-18");
        assert_eq!(negotiate_protocol_version(Some("2025-11-25")), "2025-11-25");
        // Unknown / absent -> our latest.
        assert_eq!(
            negotiate_protocol_version(Some("1999-01-01")),
            LATEST_PROTOCOL_VERSION
        );
        assert_eq!(negotiate_protocol_version(None), LATEST_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn initialize_echoes_legacy_client_version() {
        let service = SearchService::fake_with_sources();
        let response = handle_request(
            &service,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2024-11-05", "capabilities": {} }
            }),
        )
        .await
        .expect("initialize response");
        // An existing stdio client must still see its own requested revision.
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "grok-search-rs");
    }

    #[tokio::test]
    async fn initialize_declares_latest_for_unknown_version() {
        let service = SearchService::fake_with_sources();
        let response = handle_request(
            &service,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "initialize",
                "params": { "protocolVersion": "3000-01-01", "capabilities": {} }
            }),
        )
        .await
        .expect("initialize response");
        assert_eq!(
            response["result"]["protocolVersion"],
            LATEST_PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn ping_request_gets_empty_success_response() {
        let service = SearchService::fake_with_sources();
        let response = handle_request(
            &service,
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "ping"
            }),
        )
        .await
        .expect("ping response");

        assert_eq!(response["id"], 7);
        assert_eq!(response["result"], json!({}));
    }

    #[tokio::test]
    async fn web_map_returns_url_sources_without_search_metadata() {
        let service = SearchService::fake_with_sources();
        let response = handle_request(
            &service,
            json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": {
                    "name": "web_map",
                    "arguments": {
                        "url": "https://example.com",
                        "max_results": 2
                    }
                }
            }),
        )
        .await
        .expect("web_map response");

        let output = &response["result"]["structuredContent"];
        let sources = output["sources"].as_array().expect("sources");
        assert_eq!(output["sources_count"], 2);
        assert_eq!(
            sources[0],
            json!({
                "url": "https://example.com/page-0",
                "provider": "tavily"
            })
        );
        assert!(sources[0].get("title").is_none());
        assert!(sources[0].get("description").is_none());
        assert!(sources[0].get("published_date").is_none());
    }

    #[test]
    fn tools_list_descriptions_guide_routing() {
        let listed = tools_list();
        let tools = listed["tools"].as_array().expect("tools array");

        let desc = |name: &str| -> String {
            tools
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("tool {name} missing"))["description"]
                .as_str()
                .unwrap_or_else(|| panic!("tool {name} description not a string"))
                .to_string()
        };

        // web_search: discovery-type cue, explicit no-URL case; must NOT
        // recommend itself for single-page reads (that's web_fetch's job).
        let web_search = desc("web_search");
        assert!(web_search.contains("discovery"), "web_search: {web_search}");
        assert!(
            web_search.contains("don't have a specific URL"),
            "web_search: {web_search}"
        );
        assert!(
            !web_search.contains("read a single page"),
            "web_search must not claim the single-page-read role: {web_search}"
        );

        // web_fetch: targeted single-page read, names all four special
        // sources, cross-references web_search.
        let web_fetch = desc("web_fetch");
        assert!(web_fetch.contains("specific URL"), "web_fetch: {web_fetch}");
        assert!(
            web_fetch.contains("read a single page"),
            "web_fetch: {web_fetch}"
        );
        assert!(web_fetch.contains("GitHub issue"), "web_fetch: {web_fetch}");
        assert!(
            web_fetch.contains("StackOverflow") || web_fetch.contains("StackExchange"),
            "web_fetch: {web_fetch}"
        );
        assert!(web_fetch.contains("arXiv"), "web_fetch: {web_fetch}");
        assert!(web_fetch.contains("Wikipedia"), "web_fetch: {web_fetch}");
        assert!(web_fetch.contains("web_search"), "web_fetch: {web_fetch}");

        // get_sources: reuses a prior web_search session, runs no new search.
        let get_sources = desc("get_sources");
        assert!(
            get_sources.contains("session_id"),
            "get_sources: {get_sources}"
        );
        assert!(
            get_sources.contains("new search"),
            "get_sources: {get_sources}"
        );
    }
}
