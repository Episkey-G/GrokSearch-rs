# Configuration

GrokSearch-rs reads configuration from two sources, merged with the following precedence:

1. **Process environment variables** (highest — what your MCP client passes in `env`).
2. **Global TOML config file** — `$GROK_SEARCH_CONFIG` if set, otherwise `~/.config/grok-search-rs/config.toml`.
3. **Built-in defaults** (lowest).

The config file is optional; missing files are skipped silently. See the [Config file](#config-file) section below for the TOML schema. The AI provider contract is intentionally narrow: configure a Grok/OpenAI-compatible root URL and the server calls `/v1/responses`.

## Grok Responses

| Variable | Default | Description |
|---|---|---|
| `GROK_SEARCH_API_KEY` | required | Bearer token for the configured Grok-compatible gateway. |
| `GROK_SEARCH_URL` | `https://api.x.ai` | Root URL, `/v1` base URL, or endpoint-like URL. The service normalizes it to a `/v1` base. |
| `GROK_SEARCH_MODEL` | `grok-4-1-fast-reasoning` | Model sent in the Responses payload. |
| `GROK_SEARCH_WEB_SEARCH` | `true` | Sends Responses `{"type":"web_search"}`. |
| `GROK_SEARCH_X_SEARCH` | `false` | Sends Responses `{"type":"x_search"}` only when enabled. |

Boolean values accept `1`, `true`, or `yes` as enabled. Any other value is treated as disabled.

Example:

```bash
GROK_SEARCH_API_KEY=...
GROK_SEARCH_URL=https://api.modelverse.cn
GROK_SEARCH_MODEL=grok-4-1-fast-reasoning
GROK_SEARCH_X_SEARCH=false
```

The example above calls `https://api.modelverse.cn/v1/responses`.

## Tavily

| Variable | Default | Description |
|---|---|---|
| `TAVILY_API_KEY` | unset | Enables Tavily-backed source enrichment, fallback, fetch, and map. |
| `TAVILY_API_URL` | `https://api.tavily.com` | Tavily API base URL. |
| `TAVILY_ENABLED` | `true` | Optional override. Set to `false` only when you want to disable Tavily even if `TAVILY_API_KEY` is configured. |
| `GROK_SEARCH_EXTRA_SOURCES` | `3` | Adds Tavily enrichment sources after a verifiable Grok result; Firecrawl can fallback if Tavily returns none. Set `0` to disable enrichment. |
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
| `GROK_SEARCH_FETCH_MAX_CHARS` | unset | Default character cap on `web_fetch` content. Overridden per call by `max_chars`. Unset means no truncation. |

## Config file

Drop a TOML file at `~/.config/grok-search-rs/config.toml` (or any path pointed to by `GROK_SEARCH_CONFIG`) to set defaults once and skip the per-client `env` block. Process env still wins, so individual clients can override any field at runtime.

### Scaffolding the file — `--init`

```bash
grok-search-rs --init
```

This writes an annotated template at the resolved config path with **every key commented out**. The scaffold is identical in behavior to "no config file" until you uncomment lines, so it never silently changes runtime behavior. Re-running `--init` is a no-op when the file already exists; delete the file first to regenerate.

### Why two casings?

Env vars use `UPPER_CASE` because that is the Unix shell tradition (`PATH`, `HOME`, `LANG`, `AWS_REGION` …). TOML files use lowercase `snake_case` because that is the Rust ecosystem convention (`Cargo.toml`, `pyproject.toml`, Codex `~/.codex/config.toml`). `grok-search-rs` follows each convention in its native context. Mapping rule for the table below: drop the `GROK_SEARCH_` prefix where present, then lowercase the rest.

Unknown keys are rejected by the loader — typos surface as parse errors instead of silently dropping.

| TOML key | Env equivalent |
|---|---|
| `grok_api_url` | `GROK_SEARCH_URL` |
| `grok_api_key` | `GROK_SEARCH_API_KEY` |
| `grok_model` | `GROK_SEARCH_MODEL` |
| `web_search_enabled` | `GROK_SEARCH_WEB_SEARCH` |
| `x_search_enabled` | `GROK_SEARCH_X_SEARCH` |
| `tavily_api_url` | `TAVILY_API_URL` |
| `tavily_api_key` | `TAVILY_API_KEY` |
| `tavily_enabled` | `TAVILY_ENABLED` |
| `firecrawl_api_url` | `FIRECRAWL_API_URL` |
| `firecrawl_api_key` | `FIRECRAWL_API_KEY` |
| `firecrawl_enabled` | `FIRECRAWL_ENABLED` |
| `default_extra_sources` | `GROK_SEARCH_EXTRA_SOURCES` |
| `fallback_sources` | `GROK_SEARCH_FALLBACK_SOURCES` |
| `fetch_max_chars` | `GROK_SEARCH_FETCH_MAX_CHARS` |
| `cache_size` | `GROK_SEARCH_CACHE_SIZE` |
| `timeout_seconds` | `GROK_SEARCH_TIMEOUT_SECONDS` |

Example — minimum useful file:

```toml
grok_api_key   = "xai-..."
tavily_api_key = "tvly-..."
grok_model     = "grok-4-1-fast-reasoning"
```

Example — full reference:

```toml
grok_api_url          = "https://api.x.ai"
grok_api_key          = "xai-..."
grok_model            = "grok-4-1-fast-reasoning"
web_search_enabled    = true
x_search_enabled      = false
tavily_api_url        = "https://api.tavily.com"
tavily_api_key        = "tvly-..."
tavily_enabled        = true
firecrawl_api_url     = "https://api.firecrawl.dev"
firecrawl_api_key     = "fc-..."
firecrawl_enabled     = true
default_extra_sources = 3
fallback_sources      = 5
fetch_max_chars       = 200000
cache_size            = 256
timeout_seconds       = 60
```
