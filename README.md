# GrokSearch-rs

![GrokSearch-rs product banner](assets/groksearch-rs-banner.png)

**A lightweight Rust MCP server for Grok / OpenAI‑compatible web search, plus Tavily fetch/map and Firecrawl fallback.**

`grok-search-rs` is an **MCP server** — run it locally over **stdio** (your client launches it; you do not run it directly) or as a **remote Streamable HTTP** service for mobile / multi‑device access (see [self-hosting](#self-hosting-remote-http)). It exposes one set of tools (`web_search`, `get_sources`, `web_fetch`, `web_map`, `doctor`) and supports two upstream transports so you can plug into either xAI's official API or any OpenAI‑compatible relay.

---

## Features

- 🔎 **Live web search** with cited sources, cached for follow‑up `get_sources` calls. Opt‑in `include_content` enriches the top sources with full extracted text in one call.
- 📏 **Response budgeting** — `web_search` keeps responses inside agent context limits: only the top `max_inline_sources` carry inline text, a whole‑response char budget (`response_max_chars`, default 45k — sized to stay under the MCP client token ceiling after JSON serialization) trims tail sources with recovery notes, `response_format: "concise" | "detailed"` picks the payload size, and `get_sources` pages through cached sources with `offset`/`limit`. The session cache always keeps full content.
- 🧩 **Structured `web_fetch`** — GitHub issues/PRs/releases, StackExchange/MathOverflow, arXiv, and Wikipedia URLs are parsed by specialist extractors into clean Markdown (title, state/labels, release notes, accepted‑answer ordering, abstracts, vote‑sorted answers). Anything else falls back to the generic Tavily → Firecrawl chain. Output carries `source_type` and a `fallback_reason` when a specialist was skipped.
- 🔀 **Two transports** — native xAI Responses (`/v1/responses`) **or** any OpenAI‑compatible chat‑completions gateway (`/v1/chat/completions`). Pick by env vars; no flag.
- 🔐 **Optional Grok OAuth mode** — `login/status/logout` commands store a local xAI OAuth token for Responses auth, so the MCP server can run without `GROK_SEARCH_API_KEY`.
- 🌐 **Optional remote mode** — build with `--features http` to serve the same tools over **Streamable HTTP** (multi‑tenant, bring‑your‑own‑key via request headers) for mobile / multi‑device access. See [self-hosting](#self-hosting-remote-http).
- 📥 **Tavily fetch / map** for full‑text extraction and link discovery, with **Firecrawl** as automatic fallback. `TAVILY_API_KEY` accepts a comma‑separated key list — keys rotate round‑robin with automatic failover on rate/quota errors.
- 🐦 **Optional X/Twitter search** via `x_search` (Responses transport only).
- 🩺 **`doctor`** — connectivity probe + redacted config in one tool call.
- 🗂 **Single global config file** so multiple MCP clients share one set of keys.

---

## Install

```bash
npm install -g grok-search-rs
```

The npm package ships a native Rust binary; the `grok-search-rs` command is what your MCP client launches.

---

## Quick Start

**Option A — use the hosted instance (no install).** Point any MCP client that supports remote
HTTP + custom headers at the public endpoint and pass your own keys as headers:

```bash
claude mcp add --transport http grok-search https://mcp.episkeyai.com/groksearch_rs/mcp \
  --header "X-Grok-Api-Key: xai-..." \
  --header "X-Tavily-Api-Key: tvly-..."
```

The default gateway is xAI official (`api.x.ai`) — use an xAI key. For any other Grok‑compatible
gateway, add `--header "X-Grok-Base-Url: https://your-gateway.example/v1"` with a matching key, and
optionally `--header "X-Grok-Model: <model>"` (model ids are gateway‑specific). No keys are stored
server-side; best-effort availability. Prefer your own server? See [self-hosting](#self-hosting-remote-http).

**Option B — install locally (stdio).**

1. After `npm install -g grok-search-rs`, add this MCP server entry to your client config:

   ```json
   {
     "grok-search-rs": {
       "command": "grok-search-rs",
       "args": [],
       "env": {
         "GROK_SEARCH_API_KEY": "",
         "GROK_SEARCH_URL": "",
         "GROK_SEARCH_MODEL": "grok-4.20-fast",
         "TAVILY_API_KEY": "",
         "TAVILY_API_URL": "https://api.tavily.com",
         "FIRECRAWL_API_KEY": ""
       }
     }
   }
   ```

   For Codex TOML config:

   ```toml
   [mcp_servers.grok-search-rs]
   type = "stdio"
   command = "grok-search-rs"

   [mcp_servers.grok-search-rs.env]
   FIRECRAWL_API_KEY = ""
   GROK_SEARCH_API_KEY = ""
   GROK_SEARCH_MODEL = "grok-4.20-fast"
   GROK_SEARCH_URL = ""
   TAVILY_API_KEY = ""
   TAVILY_API_URL = "https://api.tavily.com"
   ```

   Put your real keys in the empty values. If your client expects a top-level `mcpServers` / `mcp_servers` object, place the `grok-search-rs` entry under that section.

2. Optional: scaffold a shared global config file instead of duplicating env blocks in every MCP client:

   ```bash
   grok-search-rs --init
   $EDITOR ~/.config/grok-search-rs/config.toml
   ```

3. Verify:

   ```text
   Ask your assistant: "call doctor"
   ```

   Successful output shows `reachable: true` for each enabled upstream and `transport: Responses` (or `ChatCompletions`).

---

## Configuration

The MCP **transport** decides how config reaches the server — same values, different channel (forced by the transport, not a project setting):

- **stdio (local):** environment variables — the `env` block in your MCP client config.
- **remote HTTP:** per-request HTTP headers — the server stores no keys.

| Setting | stdio env | remote header |
|---|---|---|
| Grok API key | `GROK_SEARCH_API_KEY` | `X-Grok-Api-Key` |
| Grok gateway URL | `GROK_SEARCH_URL` | `X-Grok-Base-Url` |
| Grok model | `GROK_SEARCH_MODEL` | `X-Grok-Model` |
| Tavily API key | `TAVILY_API_KEY` | `X-Tavily-Api-Key` |
| Firecrawl API key | `FIRECRAWL_API_KEY` | `X-Firecrawl-Api-Key` |
| GitHub token | `GITHUB_TOKEN` | `X-GitHub-Token` |

The tables below use env-key names (they also drive `config.toml` / stdio); on the remote transport send the header from the row above. Full reference and per-transport examples: [docs/CONFIGURATION.md](docs/CONFIGURATION.md#configuration-channels-stdio-env-vs-remote-headers). Both Tavily and Firecrawl keys are shared across transports.

### A. Native Grok Responses (default)

| Variable | Default | Purpose |
|---|---|---|
| `GROK_SEARCH_AUTH_MODE` | `api_key` | `api_key` uses `GROK_SEARCH_API_KEY`; `oauth` uses the local token from `grok-search-rs login`. |
| `GROK_SEARCH_API_KEY` | — *(required in `api_key` mode)* | Bearer token for the Grok / xAI gateway. |
| `GROK_SEARCH_AUTH_FILE` | `<home>/.config/grok-search-rs/auth.json` | Optional OAuth token file override. |
| `GROK_SEARCH_URL` | `https://api.x.ai` | Root, `/v1`, or full‑endpoint URL. |
| `GROK_SEARCH_MODEL` | `grok-4-1-fast-reasoning` | Model name. |
| `GROK_SEARCH_WEB_SEARCH` | `true` | Offer `web_search` tool to Grok. |
| `GROK_SEARCH_X_SEARCH` | `false` | Offer `x_search` tool (X/Twitter) to Grok. |

Verified upstream: **xAI** (`https://api.x.ai`, both tools). Other Grok‑compatible gateways work with a matching key; `x_search` availability depends on the gateway.

OAuth mode is a single-binary flow:

```bash
grok-search-rs login
grok-search-rs status
grok-search-rs logout
```

Then configure your MCP client with:

```toml
[mcp_servers.grok-search-rs]
command = "grok-search-rs"

[mcp_servers.grok-search-rs.env]
GROK_SEARCH_AUTH_MODE = "oauth"
GROK_SEARCH_MODEL = "grok-4.3"
GROK_SEARCH_WEB_SEARCH = "true"
```

OAuth mode reuses Hermes' xAI OAuth client id and stores `auth.json` locally. That may violate xAI terms or affect your account; do not share the token file. If xAI changes or blocks that OAuth flow, switch back to `api_key` mode.

### B. OpenAI‑compatible chat/completions

Activate by setting the URL **and** key while leaving `GROK_SEARCH_API_KEY` unset. Suitable for any OpenAI‑compatible relay (one‑api, vLLM, LiteLLM, marybrown, Perplexity‑style gateways, etc.).

| Variable | Default | Purpose |
|---|---|---|
| `OPENAI_COMPATIBLE_API_URL` | — | Root, `/v1`, or full‑endpoint URL. |
| `OPENAI_COMPATIBLE_API_KEY` | — | Bearer token for the relay. |
| `OPENAI_COMPATIBLE_MODEL` | falls back to `GROK_SEARCH_MODEL` | Model name to send. |

Notes:

- `GROK_SEARCH_WEB_SEARCH=true` (default) appends `tools:[{"type":"web_search"}]` to the payload. Relays that auto‑search server‑side simply ignore it.
- `GROK_SEARCH_X_SEARCH=true` is **silently ignored** on this transport (a one‑line stderr warning prints at startup). `x_search` only exists on the Responses API.
- Source extraction reads four parallel paths and de‑duplicates by URL: OpenAI `annotations[].url_citation`, Perplexity‑style `citations`, top‑level `search_sources[]`, and inline `[[n]](url)` markers.

### Tavily / Firecrawl (shared)

| Variable | Default | Purpose |
|---|---|---|
| `TAVILY_API_KEY` | — *(required for `web_fetch` / `web_map`)* | Tavily key. Comma‑separated list rotates round‑robin with failover on HTTP 401/403/429/432/433. |
| `TAVILY_API_URL` | `https://api.tavily.com` | Tavily base. |
| `GROK_SEARCH_EXTRA_SOURCES` | `3` | Extra Tavily sources after a Grok answer (`0` disables). |
| `GROK_SEARCH_FALLBACK_SOURCES` | `5` | Fallback source count when the AI step can't verify itself. |
| `FIRECRAWL_API_KEY` | unset | Enables Firecrawl as `web_fetch` / source fallback. |
| `FIRECRAWL_API_URL` | `https://api.firecrawl.dev` | Firecrawl base. |
| `GROK_SEARCH_CACHE_SIZE` | `256` | Max cached `web_search` sessions. |
| `GROK_SEARCH_TIMEOUT_SECONDS` | `60` | HTTP timeout for all upstreams. |
| `GROK_SEARCH_FETCH_MAX_CHARS` | unset | Default char cap on `web_fetch`. |
| `GROK_SEARCH_MAX_INLINE_SOURCES` | `5` | Max `web_search` sources carrying inline content; the rest are metadata‑only. |
| `GROK_SEARCH_RESPONSE_MAX_CHARS` | `45000` | Whole‑response char budget for `web_search`; over‑budget output is truncated tail‑first with `truncated: true`. Sized to keep the serialized result under the MCP client token ceiling (Claude Code default `MAX_MCP_OUTPUT_TOKENS=25000`). |

### Source extraction (`web_fetch` specialists / `web_search` enrichment)

| Variable | Default | Purpose |
|---|---|---|
| `GITHUB_TOKEN` | unset | Authenticates GitHub issue/PR/release fetches (higher API rate limit; private repos). Specialist works unauthenticated but is rate‑limited. |
| `GROK_SEARCH_SOURCE_MAX_ANSWERS` | `5` | StackExchange answers rendered before folding. |
| `GROK_SEARCH_SOURCE_MAX_COMMENTS` | `30` | GitHub / StackExchange comments rendered before folding. |
| `GROK_SEARCH_ENRICH_CONCURRENCY` | `3` | Parallel source enrichments for `web_search` `include_content` (clamped 1..5). |
| `GROK_SEARCH_ENRICH_MAX_CHARS` | `15000` | Char cap per enriched source body. |

These specialists need **no Tavily/Firecrawl key** — they hit the public GitHub,
StackExchange, arXiv, and Wikipedia APIs directly. Tavily/Firecrawl are only used
for the generic fallback path.

### Selection rules at startup

1. If `GROK_SEARCH_AUTH_MODE=oauth` → **Responses** transport with the local OAuth token.
2. Else if `GROK_SEARCH_API_KEY` is set → **Responses** transport with a static Bearer key.
3. Else if both `OPENAI_COMPATIBLE_API_URL` and `OPENAI_COMPATIBLE_API_KEY` are set → **ChatCompletions** transport.
4. Else → server fails with a clear `MissingConfig` error.

### Global config file

Tired of duplicating `env` blocks across clients? Run `grok-search-rs --init` once to scaffold `<home>/.config/grok-search-rs/config.toml`, fill in your keys, and every client can shrink to `{"command": "grok-search-rs"}`.

| Path order | Location |
|---|---|
| 1 | `$GROK_SEARCH_CONFIG` (explicit override, any platform) |
| 2 | `$HOME/.config/grok-search-rs/config.toml` (Unix / macOS / Git Bash) |
| 3 | `%USERPROFILE%\.config\grok-search-rs\config.toml` (native Windows) |

**Precedence**: per‑client `env` **>** config file **>** built‑in defaults. File keys are lowercase `snake_case` (env `GROK_SEARCH_MODEL` → file `grok_model`). Unknown keys are rejected. Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

---

## MCP Tools

| Tool | When to call it |
|---|---|
| `web_search` | Sourced summary for a topic. Sources cached for follow‑up. `response_format: "concise"` returns answer + metadata only; `"detailed"` inlines source text within the response budget. |
| `get_sources` | Re‑fetch sources of a previous `web_search` by `session_id`. Supports `offset` / `limit` pagination for large source sets. |
| `web_fetch` | Page content as clean Markdown. Specialist extractors for GitHub / StackExchange / arXiv / Wikipedia; generic Tavily → Firecrawl fallback otherwise. Returns `source_type` + `fallback_reason`. |
| `web_map` | Discover URLs on a domain via Tavily Map. |
| `doctor` | Live connectivity probe + redacted config. Run first when something looks off. |

---

## Self-hosting (remote HTTP)

Besides the default stdio mode, `grok-search-rs` can run as a **remote, multi‑tenant
Streamable HTTP MCP server** so mobile / on‑the‑go / multi‑device clients can reach it over
the network. It is **opt‑in behind the `http` Cargo feature** — the default build is
unchanged (pure stdio, no HTTP dependencies linked in).

**Bring‑your‑own‑key, zero shared credentials.** The server stores no API keys. Each
request carries the caller's own keys as headers, so many users can share one endpoint and
each pays with their own keys:

- `X-Grok-Api-Key`, `X-Tavily-Api-Key`, `X-Firecrawl-Api-Key` (optional `X-GitHub-Token`)
- Optional non‑secret overrides: `X-Grok-Base-Url` (gateway), `X-Grok-Model` (model name, since model ids are gateway‑specific)

A missing required key returns `401` (fail‑closed); OAuth is rejected on this transport
(stdio only). The operator sets the default Grok‑compatible gateway via `GROK_SEARCH_URL`
(default `https://api.x.ai`), and callers may point at any other Grok‑compatible gateway with an
`X-Grok-Base-Url` header (any public gateway is honored; internal/private addresses are rejected).
The remote transport uses the Grok **Responses** API only; the OpenAI-compatible chat-completions transport is stdio-only.

**No local build needed.** Every release ships ready‑to‑run server artifacts for Linux
`x86_64` + `aarch64` (static musl) — ideal for low‑RAM/small‑disk boards where a native
`cargo build` would OOM or fill the disk:

- **Prebuilt binary** — download `grok-search-rs-http_Linux_<arch>.tar.gz` from the
  [latest release](https://github.com/Episkey-G/GrokSearch-rs/releases/latest) and run
  `GROK_MCP_BIND=0.0.0.0:8080 ./grok-search-rs --http`. (The plain `grok-search-rs_…`
  assets are **stdio‑only**: the HTTP transport is compile‑time gated and not in them.)
- **Docker image** — multi‑arch `amd64`/`arm64`:
  `docker pull ghcr.io/episkey-g/grok-search-rs:latest` (serves on `:8080`).

Building from source instead:

```bash
cargo build --profile release-http --features http    # release-http => panic=unwind (handler panic won't kill the server)
GROK_MCP_BIND=127.0.0.1:8080 target/release-http/grok-search-rs --http   # bind loopback; terminate TLS upstream
```

Put a TLS‑terminating reverse proxy (e.g. Caddy) in front. The repo ships a `Dockerfile`,
`Dockerfile.deploy` (runtime‑only, multi‑arch `amd64`/`arm64` — cross‑compile the binaries
first with `scripts/build-deploy-dist.sh`; ideal for low‑RAM hosts), `docker-compose.yml`, and `Caddyfile`
for a one‑command deploy with automatic HTTPS. Set `MCP_HOSTNAME` (your domain or a
`<dashed-ip>.sslip.io` name) via the environment or a git‑ignored `.env` — **not** in the
repo.

Connect a client the same way as the hosted instance ([Quick Start](#quick-start)), pointing
`url` at your own host (`https://<your-host>/groksearch_rs/mcp`).

### Rotating a key

Keys are never stored server‑side, so rotation is entirely client‑side:

- **stdio** — update the key in your MCP client's `env` block (or the global `config.toml`)
  and restart the client.
- **remote HTTP** — update the header value in your client config. For Claude Code:
  ```bash
  claude mcp remove grok-search -s user
  claude mcp add --transport http grok-search https://<your-host>/groksearch_rs/mcp \
    --header "X-Grok-Api-Key: <new-key>" --header "X-Tavily-Api-Key: <new-key>"
  ```

Rotate immediately if a key was ever printed, logged, or shared.

---

## Build from source

```bash
git clone https://github.com/Episkey-G/GrokSearch-rs.git
cd GrokSearch-rs
cargo build --release
```

The binary lands at `target/release/grok-search-rs`. Point your MCP client's `command` at the absolute path.

---

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

More docs:

- [Configuration](docs/CONFIGURATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Testing](docs/TESTING.md)

---

## ⭐ Star History

<a href="https://www.star-history.com/?repos=Episkey-G%2FGrokSearch-rs&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=Episkey-G/GrokSearch-rs&type=date&theme=dark&legend=top-left&sealed_token=azObTSBD9fgG_4eDHXglTgCUguLssOhllpL_T1u5KRiuxWgqipir_p_mWXlqjtX0NJ-_JtEURaaMBYOk3_VlEqmTQJvfn6jMGoiAEFU8wXeXZT4v_dWsZA" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=Episkey-G/GrokSearch-rs&type=date&legend=top-left&sealed_token=azObTSBD9fgG_4eDHXglTgCUguLssOhllpL_T1u5KRiuxWgqipir_p_mWXlqjtX0NJ-_JtEURaaMBYOk3_VlEqmTQJvfn6jMGoiAEFU8wXeXZT4v_dWsZA" />
   <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=Episkey-G/GrokSearch-rs&type=date&legend=top-left&sealed_token=azObTSBD9fgG_4eDHXglTgCUguLssOhllpL_T1u5KRiuxWgqipir_p_mWXlqjtX0NJ-_JtEURaaMBYOk3_VlEqmTQJvfn6jMGoiAEFU8wXeXZT4v_dWsZA" />
 </picture>
</a>

---

## Acknowledgements

- Inspired by [GuDaStudio/GrokSearch](https://github.com/GuDaStudio/GrokSearch) — the original Python implementation that pioneered the Grok + Tavily + Firecrawl combo this project rewrites in Rust.
- Thanks to the [LinuxDo](https://linux.do) community for the discussions, feedback, and the prior art that inspired this rewrite.

## License

MIT — see [LICENSE](LICENSE).
