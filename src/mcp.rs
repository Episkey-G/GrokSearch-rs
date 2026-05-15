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
            let content = service.web_fetch(url).await?;
            Ok(json!({ "url": url, "content": content }))
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
            Ok(json!({ "url": url, "sources_count": sources.len(), "sources": sources }))
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
                "description": "Fetch one page through Tavily Extract, with Firecrawl scrape fallback when configured.",
                "inputSchema": {
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": { "type": "string" }
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
}
