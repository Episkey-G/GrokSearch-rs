# Configuration

GrokSearch-rs reads configuration from environment variables. The AI provider contract is intentionally narrow: configure a Grok/OpenAI-compatible root URL and the server calls `/v1/responses`.

## Grok Responses

| Variable | Default | Description |
|---|---|---|
| `GROK_SEARCH_API_KEY` | required | Bearer token for the configured Grok-compatible gateway. |
| `GROK_SEARCH_URL` | `https://api.x.ai` | Root URL, `/v1` base URL, or endpoint-like URL. The service normalizes it to a `/v1` base. |
| `GROK_SEARCH_MODEL` | `grok-4.3` | Model sent in the Responses payload. |
| `GROK_SEARCH_WEB_SEARCH` | `true` | Sends Responses `{"type":"web_search"}`. |
| `GROK_SEARCH_X_SEARCH` | `false` | Sends Responses `{"type":"x_search"}` only when enabled. |

Boolean values accept `1`, `true`, or `yes` as enabled. Any other value is treated as disabled.

Example:

```bash
GROK_SEARCH_API_KEY=...
GROK_SEARCH_URL=https://api.modelverse.cn
GROK_SEARCH_MODEL=grok-4.3
GROK_SEARCH_X_SEARCH=false
```

The example above calls `https://api.modelverse.cn/v1/responses`.

## Tavily

| Variable | Default | Description |
|---|---|---|
| `TAVILY_API_KEY` | unset | Enables Tavily-backed source enrichment, fallback, fetch, and map. |
| `TAVILY_API_URL` | `https://api.tavily.com` | Tavily API base URL. |
| `TAVILY_ENABLED` | `true` | Optional override. Set to `false` only when you want to disable Tavily even if `TAVILY_API_KEY` is configured. |
| `GROK_SEARCH_EXTRA_SOURCES` | `0` | Adds Tavily enrichment sources after a verifiable Grok result; Firecrawl can fallback if Tavily returns none. |
| `GROK_SEARCH_FALLBACK_SOURCES` | `5` | Number of fallback sources to cache when Grok is unverifiable. |

## Firecrawl

| Variable | Default | Description |
|---|---|---|
| `FIRECRAWL_API_KEY` | unset | Enables Firecrawl fallback for `web_fetch` and supplemental fallback sources. |
| `FIRECRAWL_API_URL` | `https://api.firecrawl.dev` | Firecrawl API base URL, normalized to `/v1`. |
| `FIRECRAWL_ENABLED` | `true` | Optional override. Set to `false` to disable Firecrawl even if a key is configured. |

## Cache

| Variable | Default | Description |
|---|---|---|
| `GROK_SEARCH_CACHE_SIZE` | `256` | Maximum cached search sessions for `get_sources`. |
| `GROK_SEARCH_TIMEOUT_SECONDS` | `60` | HTTP timeout for Grok, Tavily, and Firecrawl requests. |
