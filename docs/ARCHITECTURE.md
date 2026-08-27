# Architecture

GrokSearch-rs is a Rust MCP server that keeps the original GrokSearch product boundary while making provider behavior explicit and testable.

```text
MCP client
  -> default stdio JSON-RPC (src/mcp.rs)
     or optional Streamable HTTP (src/http.rs, Cargo feature `http`)
  -> shared MCP request handler
      -> src/service.rs
      -> credential provider: static API key or xAI OAuth token
      -> Grok Responses provider: /v1/responses with web_search and optional x_search
         or OpenAI-compatible provider: /v1/chat/completions (stdio only)
      -> Tavily provider: search / extract / map
      -> Firecrawl provider: search / scrape fallback
      -> source cache
```

## Product Boundary

- `web_search` is the AI search path. Grok Responses is the default; local stdio can instead select an OpenAI-compatible Chat Completions gateway.
- `get_sources` retrieves cached sources by `session_id`.
- `web_fetch` fetches page content through Tavily Extract first, then Firecrawl scrape if configured.
- `web_map` discovers URLs through Tavily Map.
- Tavily and Firecrawl are not the default answer generators inside `web_search`; they provide enrichment, fallback sources, fetch, and map capability.
- Agents should use `web_search` for concise sourced summaries, call `get_sources` before source-specific claims, citation lists, or follow-up fetches, and call `web_fetch` for exact page evidence, quotes, technical details, or when the summary is insufficient.

## Provider Layer

The service builds one internal search request, then dispatches it through the selected upstream AI transport:

| Provider | Endpoint | Tool shape |
|---|---|---|
| Grok Responses | `{GROK_SEARCH_URL normalized to /v1}/responses` | `{"type":"web_search"}` plus optional `{"type":"x_search"}` |
| OpenAI-compatible Chat Completions | `{OPENAI_COMPATIBLE_API_URL normalized to /v1}/chat/completions` | Optional `{"type":"web_search"}` hint; no `x_search` |

These are upstream provider tools. `x_search` is sent only inside an xAI
Responses request and is not an additional MCP tool exposed to clients.

The provider returns normalized assistant content and normalized `Source` values. Empty content or missing native sources are treated as unverifiable for `web_search`.

Authentication is separated from the upstream providers:

- `api_key` mode returns the configured `GROK_SEARCH_API_KEY` as a static Bearer token.
- `oauth` mode reads the local auth file, refreshes the access token when it is near expiry, and returns the fresh Bearer token for the same `/v1/responses` request body.
- the OpenAI-compatible path uses `OPENAI_COMPATIBLE_API_KEY` for its `/v1/chat/completions` request.

OAuth login is not a service boundary. `grok-search-rs login` temporarily listens on `127.0.0.1:56121` for the browser callback, stores the token file, then exits. OAuth-backed MCP operation remains stdio-only; the optional remote HTTP transport rejects OAuth.

## Source Provenance

Sources retain their origin through the `provider` field:

- `grok_responses`: native Responses citation or web search source.
- `tavily_enrichment`: supplemental Tavily source after Grok succeeds.
- `tavily_fallback`: Tavily source used because Grok failed or was unverifiable.
- `firecrawl_enrichment`: Firecrawl source used when Tavily supplemental or fallback source lookup returns nothing.
- `tavily` / `firecrawl`: direct provider source before orchestration rewrites provenance.

## Fallback Rules

`web_search` falls back to source providers when:

- the active upstream AI request fails,
- the provider response content is empty,
- the provider response has no verifiable native sources.

Fallback tries Tavily first, then Firecrawl when configured. The output exposes `search_provider`, `fallback_used`, and `fallback_reason` so MCP clients can distinguish an AI-provider result from fallback-source handling.

## MCP Transport

The default build and runtime mode are a stdio JSON-RPC server implemented in
`src/mcp.rs`. Building with Cargo feature `http` adds the optional Streamable
HTTP server in `src/http.rs`; it is selected at runtime with `--http`, `serve`,
or `GROK_MCP_BIND`, and otherwise the feature-enabled binary still starts in
stdio mode. The HTTP server exposes `POST /mcp`, accepts caller credentials in
request headers, and shares the same MCP tool handler as stdio.

Both MCP transports handle:

- `initialize`
- `tools/list`
- `tools/call`

Tool responses are serialized JSON inside MCP text content for broad client compatibility.
