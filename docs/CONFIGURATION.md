# Configuration

GrokSearch-rs reads configuration from environment variables. The provider is selected by `GROK_SEARCH_RS_PROVIDER`; set it explicitly to the protocol you want to use.

## Provider Selection

| Variable | Values | Default | Description |
|---|---|---|---|
| `GROK_SEARCH_RS_PROVIDER` | `anthropic`, `openai` | `anthropic` | Selects the provider adapter. `anthropic` calls `/messages`; `openai` calls `/responses`. |
| `GROK_SEARCH_RS_WEB_SEARCH` | boolean | `true` | Enables the provider's web search tool. |
| `GROK_SEARCH_RS_X_SEARCH` | boolean | `false` | Includes OpenAI Responses `x_search`; only meaningful for `GROK_SEARCH_RS_PROVIDER=openai`. |

Boolean values accept `1`, `true`, or `yes` as enabled. Any other value is treated as disabled.

## Anthropic Messages Provider

Use this for gateways that expose an Anthropic Messages-compatible API and support `web_search_20250305`.

```bash
GROK_SEARCH_RS_PROVIDER=anthropic
ANTHROPIC_API_KEY=...
ANTHROPIC_API_URL=http://64.186.226.237:8317/v1
ANTHROPIC_MODEL=grok-4-1-fast-reasoning
GROK_SEARCH_RS_WEB_SEARCH=true
```

Provider-specific fallback names are also accepted for migration convenience:

| Primary | Compatibility fallback |
|---|---|
| `ANTHROPIC_API_KEY` | `GROK_API_KEY`, `OPENAI_API_KEY`, `XAI_API_KEY` |
| `ANTHROPIC_API_URL` | `GROK_API_URL`, `OPENAI_API_URL`, `XAI_API_URL` |
| `ANTHROPIC_MODEL` | `GROK_MODEL`, `OPENAI_MODEL`, `XAI_MODEL` |

## OpenAI Responses Provider

Use this for native xAI or new-api routes that preserve OpenAI Responses built-in tools.

```bash
GROK_SEARCH_RS_PROVIDER=openai
OPENAI_API_KEY=...
OPENAI_API_URL=https://api.x.ai/v1
OPENAI_MODEL=grok-4.3
GROK_SEARCH_RS_WEB_SEARCH=true
GROK_SEARCH_RS_X_SEARCH=true
```

If a gateway converts `/responses` to `/chat/completions` and drops built-in tools, GrokSearch-rs will treat empty or source-less responses as unverifiable and fall back to Tavily when configured.

## Tavily

| Variable | Default | Description |
|---|---|---|
| `TAVILY_API_KEY` | unset | Enables Tavily-backed source enrichment, fallback, fetch, and map. |
| `TAVILY_API_URL` | `https://api.tavily.com` | Tavily API base URL. |
| `TAVILY_ENABLED` | `true` | Enables or disables Tavily integration. |
| `GROK_SEARCH_RS_EXTRA_SOURCES` | `0` | Adds Tavily enrichment sources after a verifiable Grok result. |
| `GROK_SEARCH_RS_FALLBACK_SOURCES` | `5` | Number of Tavily sources to cache when Grok is unverifiable. |

## Cache

| Variable | Default | Description |
|---|---|---|
| `GROK_SEARCH_RS_CACHE_SIZE` | `256` | Maximum cached search sessions for `get_sources`. |
