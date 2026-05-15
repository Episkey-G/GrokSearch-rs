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

async fn handle_message(service: &SearchService, request: Value) -> Option<Value> {
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
        "initialize" => Ok(success_response(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "grok-search-rs",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {}
                }
            }),
        )),
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
            let output = service.get_sources(session_id).await?;
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
                "description": "Search the web with Grok Responses, enriched by Tavily and falling back to Firecrawl when needed.",
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
                        }
                    }
                }
            },
            {
                "name": "get_sources",
                "description": "Return cached sources for a previous web_search session_id.",
                "inputSchema": {
                    "type": "object",
                    "required": ["session_id"],
                    "properties": {
                        "session_id": { "type": "string" }
                    }
                }
            },
            {
                "name": "web_fetch",
                "description": "Fetch one page through Tavily Extract, with Firecrawl scrape fallback when configured. Returns {url, content, original_length, truncated}.",
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

fn error_response(id: Value, code: i64, message: String) -> Value {
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
}
