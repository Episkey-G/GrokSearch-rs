# Configuration

GrokSearch-rs reads configuration from two sources, merged with the following precedence:

1. **Process environment variables** (highest — what your MCP client passes in `env`).
2. **Global TOML config file** — `$GROK_SEARCH_CONFIG` if set, otherwise `<home>/.config/grok-search-rs/config.toml` on every platform. `<home>` is `$HOME` on Unix / Git Bash, `%USERPROFILE%` on native Windows shells (PowerShell, cmd).
3. **Built-in defaults** (lowest).

The config file is optional; missing files are skipped silently. See the [Config file](#config-file) section below for the TOML schema. The AI provider contract is intentionally narrow: configure a Grok/OpenAI-compatible root URL and the server calls `/v1/responses`.

> **Configuring the remote HTTP transport?** The server stores no credentials — each request supplies its keys as **HTTP headers**, not env. The variable tables in this document use **env-key names**; the header for each is in the mapping table under [Configuration channels](#configuration-channels-stdio-env-vs-remote-headers) directly below.

## Configuration channels (stdio env vs remote headers)

Which channel carries your config is decided by the MCP **transport**, not a project setting — the two transports have no other way to receive per-instance config:

- **stdio (local):** the MCP client spawns `grok-search-rs` as a child process and can only hand it **environment variables** (the `env` block in your client config). There is no HTTP, so there are no headers.
- **Streamable HTTP (remote):** the client talks to an already-running, multi-tenant server that stores **no** credentials. The only per-request channel is **HTTP headers**; server-side env would be shared across every caller, so per-request secrets are deliberately never read from it.

Both carry the **same configuration values** — only the delivery differs. Everything else in this document uses the **env-key** name; on the remote transport, send the matching header from this table:

| Setting | stdio env key | remote HTTP header |
|---|---|---|
| Grok API key | `GROK_SEARCH_API_KEY` | `X-Grok-Api-Key` |
| Grok gateway URL | `GROK_SEARCH_URL` | `X-Grok-Base-Url` |
| Grok model | `GROK_SEARCH_MODEL` | `X-Grok-Model` |
| Tavily API key | `TAVILY_API_KEY` | `X-Tavily-Api-Key` |
| Firecrawl API key | `FIRECRAWL_API_KEY` | `X-Firecrawl-Api-Key` |
| GitHub token | `GITHUB_TOKEN` | `X-GitHub-Token` |

Only these six are accepted as headers — the caller's per-request secrets plus the gateway/model that pair with the caller's key. Most other settings here (timeouts, budgets, enrichment knobs, Tavily/Firecrawl base URLs, feature toggles) are **operator-fixed**: set once in the server's own environment, never per request. `GROK_SEARCH_URL` / `GROK_SEARCH_MODEL` have operator defaults too; the `X-Grok-Base-Url` / `X-Grok-Model` headers override them per request. **Two groups are stdio-only — stripped over HTTP, not operator-fixed:** OAuth (`GROK_SEARCH_AUTH_MODE` / `GROK_SEARCH_AUTH_FILE`) and the OpenAI-compatible chat-completions transport (`OPENAI_COMPATIBLE_API_URL` / `_API_KEY` / `_MODEL`). The remote server serves Grok **Responses** only; to run a chat-completions relay, use the stdio transport.

The header is `X-` + the env key in `Kebab-Case`, but a few names are historical (the `GROK_SEARCH_` prefix collapses to `X-Grok-`, and `GROK_SEARCH_URL` → `X-Grok-Base-Url`) — **read names off the table above rather than deriving them by hand.**

### Minimal setup — each transport

Remote (hosted or self-hosted) — headers:

```bash
claude mcp add --transport http grok-search-rs https://<host>/groksearch_rs/mcp \
  --header "X-Grok-Api-Key: <key>" \
  --header "X-Grok-Base-Url: https://<gateway>/v1" \
  --header "X-Grok-Model: <model>" \
  --header "X-Tavily-Api-Key: tvly-..."
```

Local (stdio) — the same values as `env`:

```json
{
  "mcpServers": {
    "grok-search-rs": {
      "command": "grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "<key>",
        "GROK_SEARCH_URL": "https://<gateway>/v1",
        "GROK_SEARCH_MODEL": "<model>",
        "TAVILY_API_KEY": "tvly-..."
      }
    }
  }
}
```

## Grok Responses

| Variable | Default | Description |
|---|---|---|
| `GROK_SEARCH_AUTH_MODE` | `api_key` | `api_key` uses `GROK_SEARCH_API_KEY`; `oauth` uses the local token file created by `grok-search-rs login`. |
| `GROK_SEARCH_API_KEY` | required in `api_key` mode | Bearer token for the configured Grok-compatible gateway. |
| `GROK_SEARCH_AUTH_FILE` | `<home>/.config/grok-search-rs/auth.json` | Optional OAuth token file override. |
| `GROK_SEARCH_URL` | `https://api.x.ai` | Root URL, `/v1` base URL, or endpoint-like URL. The service normalizes it to a `/v1` base. |
| `GROK_SEARCH_MODEL` | `grok-4-1-fast-reasoning` | Model sent in the Responses payload. |
| `GROK_SEARCH_WEB_SEARCH` | `true` | Sends Responses `{"type":"web_search"}`. |
| `GROK_SEARCH_X_SEARCH` | `false` | Sends Responses `{"type":"x_search"}` only when enabled. |

Boolean values accept `1`, `true`, or `yes` as enabled. Any other value is treated as disabled.

Example:

```bash
GROK_SEARCH_API_KEY=...
GROK_SEARCH_URL=https://api.x.ai
GROK_SEARCH_MODEL=grok-4-1-fast-reasoning
GROK_SEARCH_X_SEARCH=false
```

The example above calls `https://api.x.ai/v1/responses`.

### OAuth mode

OAuth mode keeps the normal Responses payload and only changes where the Bearer token comes from. The binary handles login and MCP stdio; it does not start a background HTTP proxy.

```bash
grok-search-rs login
grok-search-rs status
grok-search-rs logout
```

`login` opens xAI OAuth in a browser, listens once on `http://127.0.0.1:56121/callback`, and writes `access_token`, `refresh_token`, `id_token`, `token_endpoint`, `base_url`, and `last_refresh` to `auth.json`. `status` prints token presence, expiry, and the auth file path without printing the token. `logout` removes the local auth file.

OAuth mode reuses Hermes' xAI OAuth client id. This may violate xAI terms or create account risk, and Windows stores the token as a normal local file. Do not share the token file.

Minimal Codex config:

```toml
[mcp_servers.grok-search-rs]
command = "grok-search-rs"

[mcp_servers.grok-search-rs.env]
GROK_SEARCH_AUTH_MODE = "oauth"
GROK_SEARCH_MODEL = "grok-4.3"
GROK_SEARCH_WEB_SEARCH = "true"
```

## Tavily

| Variable | Default | Description |
|---|---|---|
| `TAVILY_API_KEY` | unset | Enables Tavily-backed source enrichment, fallback, fetch, and map. Accepts a single key or a comma-separated list (`tvly-a,tvly-b`); multiple keys rotate round-robin per request, with automatic failover to the next key on key-scoped errors (HTTP 401/403/429/432/433). |
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

## Source extraction

Specialist `web_fetch` extractors (GitHub, StackExchange, arXiv, Wikipedia) and
`web_search` inline enrichment. The specialists call public APIs directly — no
Tavily/Firecrawl key required.

| Variable | Default | Description |
|---|---|---|
| `GITHUB_TOKEN` | unset | GitHub token for issue/PR/release fetches. Anonymous works but is capped at ~60 req/hr; a token raises the limit and allows private repos. |
| `GROK_SEARCH_SOURCE_MAX_ANSWERS` | `5` | StackExchange answers rendered before the "more answers" fold. |
| `GROK_SEARCH_SOURCE_MAX_COMMENTS` | `30` | GitHub / StackExchange comments rendered before folding. |
| `GROK_SEARCH_ENRICH_CONCURRENCY` | `3` | Parallel source enrichments when `web_search` is called with `include_content: true`. Clamped to `1..=5`. |
| `GROK_SEARCH_ENRICH_MAX_CHARS` | `15000` | Character cap per enriched source body. |

## Response budget

Caps the size of a single `web_search` response so large source sets cannot
blow past MCP client context limits. The session cache always keeps full
content; truncated sources carry a note pointing at `web_fetch(url)` /
`get_sources(session_id)` for recovery.

| Variable | Default | Description |
|---|---|---|
| `GROK_SEARCH_MAX_INLINE_SOURCES` | `5` | Maximum sources that carry inline `content` per `web_search` response; the rest return metadata only. |
| `GROK_SEARCH_RESPONSE_MAX_CHARS` | `60000` | Whole-response character budget (answer + per-source metadata and inline content). Over-budget responses truncate inline content tail-first, then drop trailing sources (always keeping at least one) and set `truncated: true`. |

## Config file

Drop a TOML file at `<home>/.config/grok-search-rs/config.toml` (or any path pointed to by `GROK_SEARCH_CONFIG`) to set defaults once and skip the per-client `env` block. Process env still wins, so individual clients can override any field at runtime.

Resolved per platform:

- **macOS / Linux**: `$HOME/.config/grok-search-rs/config.toml` — e.g. `/Users/alice/.config/grok-search-rs/config.toml`.
- **Windows (PowerShell / cmd)**: `%USERPROFILE%\.config\grok-search-rs\config.toml` — e.g. `C:\Users\chen\.config\grok-search-rs\config.toml`.
- **Windows (Git Bash / MSYS)**: same as Unix — `$HOME/.config/grok-search-rs/config.toml`.

`grok-search-rs --init` picks the right path automatically; no platform-specific shell setup required.

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
| `grok_auth_mode` | `GROK_SEARCH_AUTH_MODE` |
| `grok_auth_file` | `GROK_SEARCH_AUTH_FILE` |
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
| `github_token` | `GITHUB_TOKEN` |
| `source_max_answers` | `GROK_SEARCH_SOURCE_MAX_ANSWERS` |
| `source_max_comments` | `GROK_SEARCH_SOURCE_MAX_COMMENTS` |
| `enrich_concurrency` | `GROK_SEARCH_ENRICH_CONCURRENCY` |
| `enrich_max_chars` | `GROK_SEARCH_ENRICH_MAX_CHARS` |
| `max_inline_sources` | `GROK_SEARCH_MAX_INLINE_SOURCES` |
| `response_max_chars` | `GROK_SEARCH_RESPONSE_MAX_CHARS` |

Example — minimum useful file:

```toml
grok_api_key   = "xai-..."
tavily_api_key = "tvly-..."
grok_model     = "grok-4-1-fast-reasoning"
```

Example — OAuth mode:

```toml
grok_auth_mode = "oauth"
grok_model     = "grok-4.3"
```

Example — full reference:

```toml
grok_api_url          = "https://api.x.ai"
grok_api_key          = "xai-..."
grok_auth_mode        = "api_key"
# grok_auth_file      = "C:\\Users\\chen\\.config\\grok-search-rs\\auth.json"
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
