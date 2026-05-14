use crate::error::{GrokSearchError, Result};
use crate::model::tool::WebSearchInput;
use crate::service::SearchService;
use crate::toggle::{toggle_builtin_tools_for_root, ToggleAction, ToggleHost};
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

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let response = handle_request(&service, request)
            .await
            .unwrap_or_else(|err| error_response(id, -32000, err.to_string()));
        stdout.write_all(response.to_string().as_bytes()).await?;
        stdout.write_all(b"\n").await?;
    }

    Ok(())
}

async fn handle_request(service: &SearchService, request: Value) -> Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| GrokSearchError::Parse("missing method".to_string()))?;

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
        "tools/list" => Ok(success_response(id, tools_list())),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| GrokSearchError::Parse("missing tool name".to_string()))?;
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
                    ]
                }),
            ))
        }
        _ => Err(GrokSearchError::Parse(format!(
            "unsupported method: {method}"
        ))),
    }
}

async fn call_tool(service: &SearchService, name: &str, args: Value) -> Result<Value> {
    match name {
        "health" => Ok(json!({ "status": "ok", "config": service.health() })),
        "get_config_info" => Ok(service.get_config_info()),
        "switch_model" => {
            let model = required_str(&args, "model", "switch_model.model")?;
            Ok(service.switch_model(model))
        }
        "toggle_builtin_tools" => {
            let action = parse_action(
                args.get("action")
                    .and_then(Value::as_str)
                    .unwrap_or("status"),
            );
            let host = parse_host(args.get("host").and_then(Value::as_str).unwrap_or("claude"));
            let root = std::env::current_dir()
                .map_err(|err| GrokSearchError::Provider(format!("current_dir failed: {err}")))?;
            let output = toggle_builtin_tools_for_root(&root, host, action)?;
            Ok(serde_json::to_value(output)
                .map_err(|err| GrokSearchError::Parse(format!("serialize toggle: {err}")))?)
        }
        "web_search" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| GrokSearchError::Parse("web_search.query is required".into()))?;
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
                    GrokSearchError::Parse("get_sources.session_id is required".into())
                })?;
            let output = service.get_sources(session_id).await?;
            Ok(serde_json::to_value(output)
                .map_err(|err| GrokSearchError::Parse(format!("serialize sources: {err}")))?)
        }
        "web_fetch" => {
            let url = args
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| GrokSearchError::Parse("web_fetch.url is required".into()))?;
            let content = service.web_fetch(url).await?;
            Ok(json!({ "url": url, "content": content }))
        }
        "web_map" => {
            let url = args
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| GrokSearchError::Parse("web_map.url is required".into()))?;
            let max_results = args
                .get("max_results")
                .and_then(Value::as_u64)
                .unwrap_or(10) as usize;
            let sources = service.web_map(url, max_results).await?;
            Ok(json!({ "url": url, "sources_count": sources.len(), "sources": sources }))
        }
        "plan_intent" => {
            let output = service.plan_intent(
                args.get("session_id").and_then(Value::as_str).unwrap_or(""),
                required_str(&args, "core_question", "plan_intent.core_question")?,
                required_str(&args, "query_type", "plan_intent.query_type")?,
                required_str(&args, "time_sensitivity", "plan_intent.time_sensitivity")?,
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            );
            Ok(serde_json::to_value(output).unwrap_or_else(|_| json!({})))
        }
        "plan_search" => {
            let output = service.plan_search(
                required_str(&args, "query", "plan_search.query")?,
                args.get("complexity")
                    .and_then(Value::as_str)
                    .unwrap_or("auto"),
                args.get("time_sensitivity")
                    .and_then(Value::as_str)
                    .unwrap_or("recent"),
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            );
            Ok(serde_json::to_value(output).unwrap_or_else(|_| json!({})))
        }
        "plan_complexity" => {
            let output = service.plan_complexity(
                required_str(&args, "session_id", "plan_complexity.session_id")?,
                args.get("level").and_then(Value::as_u64).unwrap_or(3) as u8,
                args.get("estimated_sub_queries")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as u32,
                args.get("estimated_tool_calls")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as u32,
                required_str(&args, "justification", "plan_complexity.justification")?,
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            );
            Ok(serde_json::to_value(output).unwrap_or_else(|_| json!({})))
        }
        "plan_sub_query" => {
            let output = service.plan_sub_query(
                required_str(&args, "session_id", "plan_sub_query.session_id")?,
                required_str(&args, "id", "plan_sub_query.id")?,
                required_str(&args, "goal", "plan_sub_query.goal")?,
                required_str(&args, "expected_output", "plan_sub_query.expected_output")?,
                required_str(&args, "boundary", "plan_sub_query.boundary")?,
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            );
            Ok(serde_json::to_value(output).unwrap_or_else(|_| json!({})))
        }
        "plan_search_term" => {
            let output = service.plan_search_term(
                required_str(&args, "session_id", "plan_search_term.session_id")?,
                required_str(&args, "term", "plan_search_term.term")?,
                required_str(&args, "purpose", "plan_search_term.purpose")?,
                args.get("round").and_then(Value::as_u64).unwrap_or(1) as u32,
                args.get("approach")
                    .and_then(Value::as_str)
                    .unwrap_or("targeted"),
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            );
            Ok(serde_json::to_value(output).unwrap_or_else(|_| json!({})))
        }
        "plan_tool_mapping" => {
            let output = service.plan_tool_mapping(
                required_str(&args, "session_id", "plan_tool_mapping.session_id")?,
                required_str(&args, "sub_query_id", "plan_tool_mapping.sub_query_id")?,
                required_str(&args, "tool", "plan_tool_mapping.tool")?,
                required_str(&args, "reason", "plan_tool_mapping.reason")?,
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            );
            Ok(serde_json::to_value(output).unwrap_or_else(|_| json!({})))
        }
        "plan_execution" => {
            let parallel = args
                .get("parallel")
                .and_then(Value::as_array)
                .map(|items| parse_parallel(items))
                .unwrap_or_default();
            let sequential = args
                .get("sequential")
                .and_then(Value::as_array)
                .map(|items| parse_string_array(items))
                .unwrap_or_default();
            let output = service.plan_execution(
                required_str(&args, "session_id", "plan_execution.session_id")?,
                parallel,
                sequential,
                args.get("estimated_rounds")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as u32,
                args.get("confidence")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            );
            Ok(serde_json::to_value(output).unwrap_or_else(|_| json!({})))
        }
        _ => Err(GrokSearchError::Parse(format!("unknown tool: {name}"))),
    }
}

fn required_str<'a>(args: &'a Value, key: &str, label: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| GrokSearchError::Parse(format!("{label} is required")))
}

fn parse_action(action: &str) -> ToggleAction {
    match action {
        "on" | "disable" => ToggleAction::On,
        "off" | "enable" => ToggleAction::Off,
        _ => ToggleAction::Status,
    }
}

fn parse_host(host: &str) -> ToggleHost {
    match host {
        "codex" => ToggleHost::Codex,
        "gemini" => ToggleHost::Gemini,
        _ => ToggleHost::Claude,
    }
}

fn parse_string_array(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn parse_parallel(values: &[Value]) -> Vec<Vec<String>> {
    values
        .iter()
        .filter_map(Value::as_array)
        .map(|items| parse_string_array(items))
        .collect()
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "health",
                "description": "Check GrokSearch-rs configuration and runtime status.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "get_config_info",
                "description": "Return redacted configuration and runtime diagnostics.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "switch_model",
                "description": "Accept a model override for compatibility; web_search also supports per-call model.",
                "inputSchema": {
                    "type": "object",
                    "required": ["model"],
                    "properties": { "model": { "type": "string" } }
                }
            },
            {
                "name": "toggle_builtin_tools",
                "description": "Disable or enable built-in web tools for claude, codex, or gemini project context.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "host": { "type": "string", "enum": ["claude", "codex", "gemini"] },
                        "action": { "type": "string", "enum": ["on", "off", "status", "enable", "disable"] }
                    }
                }
            },
            {
                "name": "web_search",
                "description": "Search with Grok Responses /v1/responses. web_search is enabled by default; x_search is controlled by GROK_SEARCH_X_SEARCH. Tavily and Firecrawl may enrich or fallback sources.",
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
                "name": "plan_search",
                "description": "One-shot planner that returns a complete search plan. Prefer this for normal use; phase tools are kept for compatibility.",
                "inputSchema": { "type": "object", "required": ["query"], "properties": {
                    "query": {"type":"string"}, "complexity": {"type":"string", "enum":["auto","simple","moderate","complex"]}, "time_sensitivity": {"type":"string"}, "confidence": {"type":"number"}
                }}
            },
            {
                "name": "plan_intent",
                "description": "Phase 1: analyze user search intent and create or update a planning session.",
                "inputSchema": { "type": "object", "required": ["core_question", "query_type", "time_sensitivity"], "properties": {
                    "session_id": {"type":"string"}, "core_question": {"type":"string"}, "query_type": {"type":"string"}, "time_sensitivity": {"type":"string"}, "confidence": {"type":"number"}
                }}
            },
            {
                "name": "plan_complexity",
                "description": "Phase 2: assess search complexity.",
                "inputSchema": { "type": "object", "required": ["session_id", "level", "justification"], "properties": {
                    "session_id": {"type":"string"}, "level": {"type":"integer"}, "estimated_sub_queries": {"type":"integer"}, "estimated_tool_calls": {"type":"integer"}, "justification": {"type":"string"}, "confidence": {"type":"number"}
                }}
            },
            {
                "name": "plan_sub_query",
                "description": "Phase 3: add one sub-query.",
                "inputSchema": { "type": "object", "required": ["session_id", "id", "goal", "expected_output", "boundary"], "properties": {
                    "session_id": {"type":"string"}, "id": {"type":"string"}, "goal": {"type":"string"}, "expected_output": {"type":"string"}, "boundary": {"type":"string"}, "confidence": {"type":"number"}
                }}
            },
            {
                "name": "plan_search_term",
                "description": "Phase 4: add one search term.",
                "inputSchema": { "type": "object", "required": ["session_id", "term", "purpose"], "properties": {
                    "session_id": {"type":"string"}, "term": {"type":"string"}, "purpose": {"type":"string"}, "round": {"type":"integer"}, "approach": {"type":"string"}, "confidence": {"type":"number"}
                }}
            },
            {
                "name": "plan_tool_mapping",
                "description": "Phase 5: map one sub-query to a tool.",
                "inputSchema": { "type": "object", "required": ["session_id", "sub_query_id", "tool", "reason"], "properties": {
                    "session_id": {"type":"string"}, "sub_query_id": {"type":"string"}, "tool": {"type":"string"}, "reason": {"type":"string"}, "confidence": {"type":"number"}
                }}
            },
            {
                "name": "plan_execution",
                "description": "Phase 6: define execution order.",
                "inputSchema": { "type": "object", "required": ["session_id"], "properties": {
                    "session_id": {"type":"string"}, "parallel": {"type":"array"}, "sequential": {"type":"array"}, "estimated_rounds": {"type":"integer"}, "confidence": {"type":"number"}
                }}
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
