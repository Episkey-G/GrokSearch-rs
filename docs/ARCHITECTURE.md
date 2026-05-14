# Architecture

GrokSearch-rs is a Rust MCP server that keeps the original GrokSearch product boundary while making provider behavior explicit and testable.

```text
MCP client
  -> src/mcp.rs
  -> src/service.rs
      -> AI provider: anthropic / openai
      -> Tavily provider: search / extract / map
      -> source cache
```

## Product Boundary

This project follows the original GrokSearch split:

- `web_search` is the AI search path. Grok is primary.
- `get_sources` retrieves cached sources by `session_id`.
- `web_fetch` fetches page content through Tavily Extract.
- `web_map` discovers URLs through Tavily Map.
- Tavily is not the default answer generator inside `web_search`; it is used for enrichment or fallback.

## Provider Layer

The service builds an internal Anthropic-style request and delegates to one provider:

| Provider | Endpoint | Tool shape |
|---|---|---|
| `anthropic` | `{ANTHROPIC_API_URL}/messages` | `{"type":"web_search_20250305","name":"web_search"}` |
| `openai` | `{OPENAI_API_URL}/responses` | `{"type":"web_search"}` plus optional `{"type":"x_search"}` |

The provider returns normalized assistant content and normalized `Source` values. Empty content or missing sources are treated as unverifiable for `web_search`.

## Source Provenance

Sources retain their origin through the `provider` field:

- `anthropic_web_search`: native provider web search source.
- `xai_web_search`: OpenAI Responses citation source.
- `tavily_enrichment`: configured supplemental Tavily source after Grok succeeds.
- `tavily_fallback`: Tavily source used because Grok failed or was unverifiable.
- `tavily`: direct Tavily provider source before orchestration rewrites provenance.

## Fallback Rules

`web_search` falls back to Tavily when:

- the AI provider request fails,
- the provider response content is empty,
- the provider response has no verifiable native sources.

The output exposes `search_provider`, `fallback_used`, and `fallback_reason` so MCP clients can distinguish a native Grok result from Tavily fallback.

## MCP Transport

The binary is a stdio JSON-RPC server. It handles:

- `initialize`
- `tools/list`
- `tools/call`

Tool responses are serialized JSON inside MCP text content for broad client compatibility.
