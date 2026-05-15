# GrokSearch-rs

![GrokSearch-rs product banner](assets/groksearch-rs-banner.png)

<p align="center">
  <b>A lightweight Rust MCP server bundling Grok web search + Tavily fetch/map + Firecrawl fallback.</b>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/grok-search-rs"><img src="https://img.shields.io/npm/v/grok-search-rs?label=npm&color=informational" alt="npm"></a>
  <a href="https://github.com/Episkey-G/GrokSearch-rs/releases"><img src="https://img.shields.io/github/v/release/Episkey-G/GrokSearch-rs?display_name=tag&sort=semver" alt="release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/Episkey-G/GrokSearch-rs" alt="license"></a>
</p>

---

> Drop one MCP server into Claude / Codex / Gemini / Cursor / VS Code / Windsurf and your assistant gets **Grok‑powered search**, **structured fetch**, and **site mapping**.

## ✨ Features

- 🔎 **Grok Responses search** — concise answer + cited sources, cached for follow‑ups.
- 📥 **Tavily fetch / map** — full‑text extract and link discovery with one call.
- 🛟 **Firecrawl fallback** — when Tavily refuses or returns thin content.
- 🩺 **Doctor probe** — `doctor` tool reports connectivity + redacted config in seconds.
- 📦 **One‑line install** — `npm install -g grok-search-rs` for the lean native binary; `npx grok-search-rs` also works (keeps Node resident as a stdio shim).

---

## 🚀 Install

You need at least:

| Key | Required | Where to get it |
| --- | --- | --- |
| `GROK_SEARCH_API_KEY` | ✅ | <https://x.ai/api> (any Grok‑compatible endpoint works) |
| `TAVILY_API_KEY`      | ✅ | <https://tavily.com> |
| `FIRECRAWL_API_KEY`   | optional | <https://firecrawl.dev> (fetch fallback) |

> 💡 **Recommended**: install once globally, then point your MCP client at the native binary directly. This skips the Node `npx` wrapper (~30–50 MB resident) and gives you the true single‑digit‑MB Rust process.
>
> ```bash
> npm install -g grok-search-rs
> which grok-search-rs   # macOS / Linux — copy the printed path
> # Windows: where grok-search-rs
> ```

Pick your client:

<details open>
<summary><b>Claude Code</b> — one command (recommended, native binary)</summary>

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

</details>

<details>
<summary><b>Codex CLI</b> — edit <code>~/.codex/config.toml</code></summary>

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

</details>

<details>
<summary><b>Gemini CLI</b> — edit <code>~/.gemini/settings.json</code></summary>

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

</details>

<details>
<summary><b>Cursor</b> — edit <code>~/.cursor/mcp.json</code> (or project <code>.cursor/mcp.json</code>)</summary>

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

</details>

<details>
<summary><b>VS Code</b> — <code>.vscode/mcp.json</code></summary>

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

</details>

<details>
<summary><b>Windsurf</b> — <code>~/.codeium/windsurf/mcp_config.json</code></summary>

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

</details>

> ⚠️ `grok-search-rs` / `npx grok-search-rs` is **not meant to be launched directly**. It speaks MCP over stdio — your client launches it. Running it in a terminal prints an onboarding guide.

---

## 🪄 Install via AI prompt

Paste this into your AI assistant — it'll detect the client and wire up the config:

```text
Install the grok-search-rs MCP server (npx -y grok-search-rs) into my current client, ask me for GROK_SEARCH_API_KEY and TAVILY_API_KEY, then call doctor to verify. Docs: https://github.com/Episkey-G/GrokSearch-rs#readme
```

---

## 🧰 Tools

| Tool | When to call it |
| --- | --- |
| `web_search`  | Concise sourced summary for a topic. Cache stores the source list for follow‑ups. |
| `get_sources` | Pull the full source URLs/snippets of a previous `web_search`. |
| `web_fetch`   | Need the *actual* page content — quotes, exact numbers, technical detail. Tavily Extract, falls back to Firecrawl. |
| `web_map`     | Discover URLs on a domain via Tavily Map. Returns `{url, provider}` only. |
| `doctor`      | Live connectivity probe + redacted config dump. Use it first when something looks off. |

Rule of thumb: **`web_search` returns answer + sources inline every call; use `web_fetch` for exact evidence and `web_map` for URL discovery. `get_sources` only re-fetches an earlier session's cache.**

### Search modes — `web_search` vs `x_search`

`grok-search-rs` enables Grok's **`web_search` tool by default** — every `web_search` MCP call hits the open web through Grok Responses and is supplemented by Tavily.

Grok's **`x_search` tool** (search inside X/Twitter) is **opt‑in** — set `GROK_SEARCH_X_SEARCH=true` to enable it alongside web search. Both tools are then offered to Grok and it picks per query.

```bash
# Default — web only
GROK_SEARCH_WEB_SEARCH=true   # already the default, shown for clarity
GROK_SEARCH_X_SEARCH=false    # default

# Enable X/Twitter search in addition to the open web
GROK_SEARCH_X_SEARCH=true
```

> Note: `x_search` requires your Grok endpoint / upstream proxy to actually expose the `x_search` tool type. xAI's official API does; some relays may strip it.

---

## ⚙️ Configuration

Minimal `.env` (if running the binary yourself):

```bash
GROK_SEARCH_API_KEY=xai-...
GROK_SEARCH_URL=https://api.x.ai
GROK_SEARCH_MODEL=grok-4-1-fast-reasoning
TAVILY_API_KEY=tvly-...

# Optional: Firecrawl fallback for fetch.
FIRECRAWL_API_KEY=fc-...
```

Optional knobs:

```bash
GROK_SEARCH_WEB_SEARCH=true
GROK_SEARCH_X_SEARCH=false
GROK_SEARCH_EXTRA_SOURCES=3
GROK_SEARCH_FALLBACK_SOURCES=5
GROK_SEARCH_TIMEOUT_SECONDS=60
GROK_SEARCH_CACHE_SIZE=256

TAVILY_API_URL=https://api.tavily.com
FIRECRAWL_API_URL=https://api.firecrawl.dev
```

`GROK_SEARCH_URL` accepts a root URL or a `/v1` URL — the server calls `/v1/responses` automatically.

Full list: [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

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
