# GrokSearch-rs

GrokSearch-rs product banner

**A lightweight Rust MCP server bundling Grok web search + Tavily fetch/map + Firecrawl fallback.**



---

> Drop one MCP server into Claude / Codex / Gemini / Cursor / VS Code / Windsurf and your assistant gets **Grok‑powered search**, **structured fetch**, and **site mapping**.

`grok-search-rs` is an **MCP server** — your client launches it over stdio, not HTTP.

## ✨ Features

- 🔎 **Grok Responses search** — concise answer + cited sources, cached for follow‑ups.
- 📥 **Tavily fetch / map** — full‑text extract and link discovery.
- 🛟 **Firecrawl fallback** — kicks in when Tavily refuses or returns thin content.
- 🐦 **Optional X/Twitter search** — one env var to add `x_search`.
- 🩺 **Doctor probe** — connectivity + redacted config in one tool call.
- 📦 **One‑line install** — `npm install -g grok-search-rs`.

---

## 🚀 Install

You need at least:


| Key                   | Required | Where to get it                                                           |
| --------------------- | -------- | ------------------------------------------------------------------------- |
| `GROK_SEARCH_API_KEY` | ✅        | [https://x.ai/api](https://x.ai/api) (any Grok‑compatible endpoint works) |
| `TAVILY_API_KEY`      | ✅        | [https://tavily.com](https://tavily.com)                                  |
| `FIRECRAWL_API_KEY`   | optional | [https://firecrawl.dev](https://firecrawl.dev) (fetch fallback)           |


> 💡 **Recommended**: install once globally, then point your MCP client at the native binary directly. This skips the Node `npx` wrapper (~30–50 MB resident) and gives you the true single‑digit‑MB Rust process.
>
> ```bash
> npm install -g grok-search-rs
> ```

Pick your client:

**Claude Code** — one command (recommended, native binary)

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

Prefer `npx` (no global install, but Node stays resident):

```bash
claude mcp add-json grok-search-rs --scope user '{
  "type": "stdio",
  "command": "npx",
  "args": ["-y", "grok-search-rs"],
  "env": {
    "GROK_SEARCH_API_KEY": "xai-...",
    "TAVILY_API_KEY": "tvly-..."
  }
}'
```



**Codex CLI** — edit `~/.codex/config.toml`

Recommended (native binary):

```toml
[mcp_servers.grok-search-rs]
command = "grok-search-rs"
env = { GROK_SEARCH_API_KEY = "xai-...", TAVILY_API_KEY = "tvly-..." }
```

Or via npx:

```toml
[mcp_servers.grok-search-rs]
command = "npx"
args = ["-y", "grok-search-rs"]
env = { GROK_SEARCH_API_KEY = "xai-...", TAVILY_API_KEY = "tvly-..." }
```



**Gemini CLI** — edit `~/.gemini/settings.json`

```json
{
  "mcpServers": {
    "grok-search-rs": {
      "command": "grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "xai-...",
        "TAVILY_API_KEY": "tvly-..."
      }
    }
  }
}
```

Swap `"command": "grok-search-rs"` for `"command": "npx", "args": ["-y", "grok-search-rs"]` if you'd rather not install globally.



**Cursor** — edit `~/.cursor/mcp.json` (or project `.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "grok-search-rs": {
      "command": "grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "xai-...",
        "TAVILY_API_KEY": "tvly-..."
      }
    }
  }
}
```



**VS Code** — `.vscode/mcp.json`

```json
{
  "servers": {
    "grok-search-rs": {
      "type": "stdio",
      "command": "grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "xai-...",
        "TAVILY_API_KEY": "tvly-..."
      }
    }
  }
}
```



**Windsurf** — `~/.codeium/windsurf/mcp_config.json`

```json
{
  "mcpServers": {
    "grok-search-rs": {
      "command": "grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "xai-...",
        "TAVILY_API_KEY": "tvly-..."
      }
    }
  }
}
```



> ⚠️ `grok-search-rs` / `npx grok-search-rs` is **not meant to be launched directly**. It speaks MCP over stdio — your client launches it. Running it in a terminal prints an onboarding guide.

---

## 🪄 Install via AI prompt

Paste this into your AI assistant — it'll detect the client and wire up the config:

```text
Install the grok-search-rs MCP server (npx -y grok-search-rs) into my current client, ask me for GROK_SEARCH_API_KEY and TAVILY_API_KEY, then call doctor to verify. Docs: https://github.com/Episkey-G/GrokSearch-rs#readme
```

---

## 🧰 MCP Tools


| Tool          | When to call it                                                                      |
| ------------- | ------------------------------------------------------------------------------------ |
| `web_search`  | Sourced summary for a topic. Sources are cached for follow‑ups.                      |
| `get_sources` | Re‑fetch sources of a previous `web_search` by `session_id`.                         |
| `web_fetch`   | Page content — quotes, exact numbers, technical detail. Tavily → Firecrawl fallback. |
| `web_map`     | Discover URLs on a domain via Tavily Map.                                            |
| `doctor`      | Live connectivity probe + redacted config. Run first when something looks off.       |


---

## 🌐 Upstream protocol


| MCP tool     | Upstream              | Endpoint                              | Fallback                       |
| ------------ | --------------------- | ------------------------------------- | ------------------------------ |
| `web_search` | Grok (xAI‑compatible) | `POST {GROK_SEARCH_URL}/v1/responses` | Tavily / Firecrawl for sources |
| `web_fetch`  | Tavily                | `POST {TAVILY_API_URL}/extract`       | Firecrawl `/v1/scrape`         |
| `web_map`    | Tavily                | `POST {TAVILY_API_URL}/map`           | —                              |
| `doctor`     | all three             | probes each                           | —                              |


`grok-search-rs` always calls the **Responses API** (`/v1/responses`), not `/v1/chat/completions`. Your upstream must implement that endpoint and accept the `web_search` / `x_search` tool types.

`GROK_SEARCH_URL` accepts the root URL, a `/v1` base, or a full endpoint — all normalized to `/v1` internally. Verified upstreams: **xAI** (`https://api.x.ai`, both tools), **Modelverse** (`https://api.modelverse.cn`, `x_search` depends on relay).

---

## 🔍 Search modes — `web_search` vs `x_search`

Grok Responses exposes two search tool types; each can be toggled independently. The MCP tool name your client sees (`web_search`) stays the same.


| Env var                  | Default | Effect                                            |
| ------------------------ | ------- | ------------------------------------------------- |
| `GROK_SEARCH_WEB_SEARCH` | `true`  | Offer Grok the `web_search` tool (open web).      |
| `GROK_SEARCH_X_SEARCH`   | `false` | Offer Grok the `x_search` tool (X/Twitter posts). |


When both are on, Grok picks per query — factual queries lean web, "what are people on X saying…" leans X.

```bash
GROK_SEARCH_X_SEARCH=true   # enable X search
```

Restart your MCP client, then verify with `doctor` (`x_search_enabled: true`).

> ⚠️ `x_search` requires the upstream to expose the `x_search` tool type. xAI's official API does; some relays strip it.

---

## ⚙️ Configuration

All config is via env vars, grouped by upstream.

### Grok Responses (required)


| Variable                 | Default                   | Purpose                            |
| ------------------------ | ------------------------- | ---------------------------------- |
| `GROK_SEARCH_API_KEY`    | — *(required)*            | Bearer token for the Grok gateway. |
| `GROK_SEARCH_URL`        | `https://api.x.ai`        | Root, `/v1`, or full‑endpoint URL. |
| `GROK_SEARCH_MODEL`      | `grok-4-1-fast-reasoning` | Model name.                        |
| `GROK_SEARCH_WEB_SEARCH` | `true`                    | Offer `web_search` to Grok.        |
| `GROK_SEARCH_X_SEARCH`   | `false`                   | Offer `x_search` to Grok.          |


### Tavily (required for `web_fetch` / `web_map`)


| Variable                       | Default                  | Purpose                                                  |
| ------------------------------ | ------------------------ | -------------------------------------------------------- |
| `TAVILY_API_KEY`               | — *(required)*           | Tavily key.                                              |
| `TAVILY_API_URL`               | `https://api.tavily.com` | Tavily base.                                             |
| `TAVILY_ENABLED`               | `true`                   | Force‑disable even with a key.                           |
| `GROK_SEARCH_EXTRA_SOURCES`    | `3`                      | Extra Tavily sources after a Grok answer (`0` disables). |
| `GROK_SEARCH_FALLBACK_SOURCES` | `5`                      | Fallback source count when Grok can't verify itself.     |


### Firecrawl (optional fallback)


| Variable            | Default                     | Purpose                                    |
| ------------------- | --------------------------- | ------------------------------------------ |
| `FIRECRAWL_API_KEY` | unset                       | Enables Firecrawl as `web_fetch` fallback. |
| `FIRECRAWL_API_URL` | `https://api.firecrawl.dev` | Firecrawl base.                            |
| `FIRECRAWL_ENABLED` | `true`                      | Force‑disable even with a key.             |


### Runtime


| Variable                      | Default | Purpose                                                          |
| ----------------------------- | ------- | ---------------------------------------------------------------- |
| `GROK_SEARCH_CACHE_SIZE`      | `256`   | Max cached `web_search` sessions.                                |
| `GROK_SEARCH_TIMEOUT_SECONDS` | `60`    | HTTP timeout for all upstreams.                                  |
| `GROK_SEARCH_FETCH_MAX_CHARS` | unset   | Default char cap on `web_fetch`; per‑call `max_chars` overrides. |


> Boolean env vars: `1` / `true` / `yes` = on; anything else = off.

### Minimal `.env`

```bash
GROK_SEARCH_API_KEY=xai-...
TAVILY_API_KEY=tvly-...
FIRECRAWL_API_KEY=fc-...        # optional
GROK_SEARCH_X_SEARCH=true       # optional
```

Full reference: [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

---

## 🏗 Build from source

```bash
git clone https://github.com/Episkey-G/GrokSearch-rs.git
cd GrokSearch-rs
cargo build --release
```

MCP entry for the local binary:

```json
{
  "mcpServers": {
    "grok-search-rs": {
      "type": "stdio",
      "command": "/absolute/path/to/target/release/grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "xai-...",
        "TAVILY_API_KEY": "tvly-..."
      }
    }
  }
}
```

---

## 🧪 Development

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

## 🙏 Acknowledgements

- Inspired by [GuDaStudio/GrokSearch](https://github.com/GuDaStudio/GrokSearch) — the original Python implementation that pioneered the Grok + Tavily + Firecrawl combo this project rewrites in Rust.
- Thanks to the [LinuxDo](https://linux.do) community for the discussions, feedback, and the prior art that inspired this rewrite.

## 📜 License

MIT — see [LICENSE](LICENSE).