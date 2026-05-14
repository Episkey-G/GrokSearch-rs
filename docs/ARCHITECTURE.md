# Architecture

GrokSearch-rs is a Rust MCP server that keeps the original GrokSearch product boundary while making provider behavior explicit and testable.

```text
MCP client
  -> src/mcp.rs
  -> src/service.rs
      -> Grok Responses provider: /v1/responses with web_search and optional x_search
      -> Tavily provider: search / extract / map
      -> Firecrawl provider: search / scrape fallback
      -> source cache
```

## Product Boundary

- `web_search` is the AI search path. Grok Responses is primary.
- `get_sources` retrieves cached sources by `session_id`.
- `web_fetch` fetches page content through Tavily Extract first, then Firecrawl scrape if configured.
- `web_map` discovers URLs through Tavily Map.
- Tavily and Firecrawl are not the default answer generators inside `web_search`; they provide enrichment, fallback sources, fetch, and map capability.
- Agents should use `web_search` for concise sourced summaries, call `get_sources` before source-specific claims, citation lists, or follow-up fetches, and call `web_fetch` for exact page evidence, quotes, technical details, or when the summary is insufficient.

## Provider Layer

The service builds an internal search request and sends one Responses payload:

| Provider | Endpoint | Tool shape |
|---|---|---|
| Grok Responses | `{GROK_SEARCH_URL normalized to /v1}/responses` | `{"type":"web_search"}` plus optional `{"type":"x_search"}` |

The provider returns normalized assistant content and normalized `Source` values. Empty content or missing native sources are treated as unverifiable for `web_search`.

## Source Provenance

Sources retain their origin through the `provider` field:

- `grok_responses`: native Responses citation or web search source.
- `tavily_enrichment`: supplemental Tavily source after Grok succeeds.
- `tavily_fallback`: Tavily source used because Grok failed or was unverifiable.
- `firecrawl_enrichment`: Firecrawl source used when Tavily supplemental or fallback source lookup returns nothing.
- `tavily` / `firecrawl`: direct provider source before orchestration rewrites provenance.

## Fallback Rules

`web_search` falls back to source providers when:

- the Grok Responses request fails,
- the provider response content is empty,
- the provider response has no verifiable native sources.

Fallback tries Tavily first, then Firecrawl when configured. The output exposes `search_provider`, `fallback_used`, and `fallback_reason` so MCP clients can distinguish a native Grok result from fallback-source handling.

## MCP Transport

The binary is a stdio JSON-RPC server. It handles:

- `initialize`
- `tools/list`
- `tools/call`

Tool responses are serialized JSON inside MCP text content for broad client compatibility.
