# Architecture

GrokSearch-rs is a Rust MCP server that keeps the original GrokSearch product boundary while making provider behavior explicit and testable.

```text
MCP client
  -> src/mcp.rs
      -> src/service.rs
      -> credential provider: static API key or xAI OAuth token
      -> Grok Responses provider: /v1/responses with web_search and optional x_search
      -> source chain (ordered; default tavily -> exa -> tinyfish -> firecrawl)
           -> Tavily provider: search / extract / map
           -> Exa provider: semantic search / contents fetch
           -> TinyFish provider: search / JS-rendering fetch
           -> Firecrawl provider: search / scrape fallback
      -> source cache
```

## Product Boundary

- `web_search` is the AI search path. Grok Responses is primary.
- `get_sources` retrieves cached sources by `session_id`.
- `web_fetch` fetches page content through the source chain in order (Tavily Extract, Exa contents, TinyFish fetch, Firecrawl scrape — whichever are configured; first non-empty page wins).
- `web_map` discovers URLs through Tavily Map (the only provider with a real site-map endpoint).
- Source providers are not the default answer generators inside `web_search`; they provide enrichment, fallback sources, fetch, and map capability. The chain order is canonical (`tavily, exa, tinyfish, firecrawl`) over configured providers, or exactly `GROK_SEARCH_SOURCE_PROVIDERS` when set.
- Agents should use `web_search` for concise sourced summaries, call `get_sources` before source-specific claims, citation lists, or follow-up fetches, and call `web_fetch` for exact page evidence, quotes, technical details, or when the summary is insufficient.

## Provider Layer

The service builds an internal search request and sends one Responses payload:

| Provider | Endpoint | Tool shape |
|---|---|---|
| Grok Responses | `{GROK_SEARCH_URL normalized to /v1}/responses` | `{"type":"web_search"}` plus optional `{"type":"x_search"}` |

The provider returns normalized assistant content and normalized `Source` values. Empty content or missing native sources are treated as unverifiable for `web_search`.

Authentication is separated from the Responses provider:

- `api_key` mode returns the configured `GROK_SEARCH_API_KEY` as a static Bearer token.
- `oauth` mode reads the local auth file, refreshes the access token when it is near expiry, and returns the fresh Bearer token for the same `/v1/responses` request body.

OAuth login is not a service boundary. `grok-search-rs login` temporarily listens on `127.0.0.1:56121` for the browser callback, stores the token file, then exits. Normal MCP operation remains stdio only.

## Source Provenance

Sources retain their origin through the `provider` field:

- `grok_responses`: native Responses citation or web search source.
- `{provider}_enrichment` (`tavily_enrichment`, `exa_enrichment`, `tinyfish_enrichment`, `firecrawl_enrichment`): supplemental source after Grok succeeds, named for the chain provider that served it.
- `{provider}_fallback` (`tavily_fallback`, `exa_fallback`, `tinyfish_fallback`, `firecrawl_fallback`): source used because Grok failed or was unverifiable, named for the chain provider that served it.
- `tavily` / `exa` / `tinyfish` / `firecrawl`: direct provider source before orchestration rewrites provenance.

## Fallback Rules

`web_search` falls back to source providers when:

- the Grok Responses request fails,
- the provider response content is empty,
- the provider response has no verifiable native sources.

Fallback walks the source chain in order; the first provider with usable results serves the response. Providers that cannot honor domain/recency filters (Firecrawl) are skipped for filtered requests instead of silently violating the filter contract. The output exposes `search_provider`, `fallback_used`, and `fallback_reason` so MCP clients can distinguish a native Grok result from fallback-source handling.

## MCP Transport

The binary is a stdio JSON-RPC server. It handles:

- `initialize`
- `tools/list`
- `tools/call`

Tool responses are serialized JSON inside MCP text content for broad client compatibility.
