# GrokSearch-rs

![GrokSearch-rs product banner](assets/groksearch-rs-banner.png)

**A lightweight Rust MCP server for Grok / OpenAI‑compatible web search, plus Tavily fetch/map and Firecrawl fallback.**

`grok-search-rs` is an **MCP stdio server** — your client (Claude Code, Codex, Cursor, VS Code, …) launches it; you do not run it directly. It exposes one set of tools (`web_search`, `get_sources`, `web_fetch`, `web_map`, `doctor`) and supports two upstream transports so you can plug into either xAI's official API or any OpenAI‑compatible relay.

---

## Features

- 🔎 **Live web search** with cited sources, cached for follow‑up `get_sources` calls.
- 🔀 **Two transports** — native xAI Responses (`/v1/responses`) **or** any OpenAI‑compatible chat‑completions gateway (`/v1/chat/completions`). Pick by env vars; no flag.
- 📥 **Tavily fetch / map** for full‑text extraction and link discovery, with **Firecrawl** as automatic fallback.
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

1. Scaffold a global config file (one‑time, optional):

   ```bash
   grok-search-rs --init
   $EDITOR ~/.config/grok-search-rs/config.toml
   ```

2. Wire it into one MCP client. Example for Claude Code:

   ```bash
   claude mcp add-json grok-search-rs --scope user '{
     "type": "stdio",
     "command": "grok-search-rs",
     "env": {
       "GROK_SEARCH_API_KEY": "xai-...",
       "TAVILY_API_KEY": "tvly-..."
     }
   }'
   ```

   For other clients (Codex / Cursor / Gemini / VS Code / Windsurf), use the same JSON shape inside their MCP config file. If you ran `--init` and put your keys in the config file, the `env` block can be omitted.

3. Verify:

   ```text
   Ask your assistant: "call doctor"
   ```

   Successful output shows `reachable: true` for each enabled upstream and `transport: Responses` (or `ChatCompletions`).

---

## Configuration

Pick **one** transport group. Both Tavily and Firecrawl keys are shared across transports.

### A. Native Grok Responses (default)

| Variable | Default | Purpose |
|---|---|---|
| `GROK_SEARCH_API_KEY` | — *(required)* | Bearer token for the Grok / xAI gateway. |
| `GROK_SEARCH_URL` | `https://api.x.ai` | Root, `/v1`, or full‑endpoint URL. |
| `GROK_SEARCH_MODEL` | `grok-4-1-fast-reasoning` | Model name. |
| `GROK_SEARCH_WEB_SEARCH` | `true` | Offer `web_search` tool to Grok. |
| `GROK_SEARCH_X_SEARCH` | `false` | Offer `x_search` tool (X/Twitter) to Grok. |

Verified upstreams: **xAI** (`https://api.x.ai`, both tools), **Modelverse** (`https://api.modelverse.cn`, `x_search` depends on relay).

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
| `TAVILY_API_KEY` | — *(required for `web_fetch` / `web_map`)* | Tavily key. |
| `TAVILY_API_URL` | `https://api.tavily.com` | Tavily base. |
| `GROK_SEARCH_EXTRA_SOURCES` | `3` | Extra Tavily sources after a Grok answer (`0` disables). |
| `GROK_SEARCH_FALLBACK_SOURCES` | `5` | Fallback source count when the AI step can't verify itself. |
| `FIRECRAWL_API_KEY` | unset | Enables Firecrawl as `web_fetch` / source fallback. |
| `FIRECRAWL_API_URL` | `https://api.firecrawl.dev` | Firecrawl base. |
| `GROK_SEARCH_CACHE_SIZE` | `256` | Max cached `web_search` sessions. |
| `GROK_SEARCH_TIMEOUT_SECONDS` | `60` | HTTP timeout for all upstreams. |
| `GROK_SEARCH_FETCH_MAX_CHARS` | unset | Default char cap on `web_fetch`. |

### Selection rules at startup

1. If `GROK_SEARCH_API_KEY` is set → **Responses** transport.
2. Else if both `OPENAI_COMPATIBLE_API_URL` and `OPENAI_COMPATIBLE_API_KEY` are set → **ChatCompletions** transport.
3. Else → server fails with a clear `MissingConfig` error.

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
| `web_search` | Sourced summary for a topic. Sources cached for follow‑up. |
| `get_sources` | Re‑fetch sources of a previous `web_search` by `session_id`. |
| `web_fetch` | Page content (Tavily → Firecrawl fallback). |
| `web_map` | Discover URLs on a domain via Tavily Map. |
| `doctor` | Live connectivity probe + redacted config. Run first when something looks off. |

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

<a href="https://www.star-history.com/?repos=Episkey-G%2FGrokSearch-rs&type=Date">
  <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=Episkey-G/GrokSearch-rs&type=Date" />
</a>

---

## Acknowledgements

- Inspired by [GuDaStudio/GrokSearch](https://github.com/GuDaStudio/GrokSearch) — the original Python implementation that pioneered the Grok + Tavily + Firecrawl combo this project rewrites in Rust.
- Thanks to the [LinuxDo](https://linux.do) community for the discussions, feedback, and the prior art that inspired this rewrite.

## License

MIT — see [LICENSE](LICENSE).
