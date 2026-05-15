# GrokSearch-rs

![GrokSearch-rs product banner](assets/groksearch-rs-banner.png)

A lightweight Rust MCP server for Grok Responses web search, Tavily fetch/map, and Firecrawl fetch fallback.

## What it does

```text
web_search -> Grok Responses /v1/responses, returns answer summary + sources
get_sources -> returns cached source URLs for a search session
web_fetch  -> Tavily Extract, fallback to Firecrawl
web_map    -> Tavily Map
```

`web_search` is for concise sourced summaries. Use `get_sources` before source-specific claims, citation lists, or follow-up fetches. Use `web_fetch` only for exact page evidence, quotes, technical details, or when the summary is insufficient.

## Quick Start

Use with Claude Code via `npx`:

```bash
claude mcp add-json grok-search-rs --scope user '{
  "type": "stdio",
  "command": "npx",
  "args": ["grok-search-rs"],
  "env": {
    "GROK_SEARCH_API_KEY": "your-grok-compatible-key",
    "GROK_SEARCH_URL": "https://api.x.ai",
    "GROK_SEARCH_MODEL": "grok-4-1-fast-reasoning",
    "TAVILY_API_KEY": "your-tavily-key"
  }
}'
```

Or run directly:

```bash
npx grok-search-rs
```

## Build from source

```bash
git clone https://github.com/Episkey-G/GrokSearch-rs.git
cd GrokSearch-rs
cargo build --release
```

## Configuration

Minimal `.env`:

```bash
GROK_SEARCH_API_KEY=your-grok-compatible-key
GROK_SEARCH_URL=https://api.x.ai
GROK_SEARCH_MODEL=grok-4-1-fast-reasoning

TAVILY_API_KEY=your-tavily-key

# Optional: enables Firecrawl fallback for fetch/source fallback.
FIRECRAWL_API_KEY=your-firecrawl-key
```

Optional settings:

```bash
GROK_SEARCH_WEB_SEARCH=true
GROK_SEARCH_X_SEARCH=false
GROK_SEARCH_EXTRA_SOURCES=0
GROK_SEARCH_FALLBACK_SOURCES=5
GROK_SEARCH_TIMEOUT_SECONDS=60
GROK_SEARCH_CACHE_SIZE=256

TAVILY_API_URL=https://api.tavily.com
FIRECRAWL_API_URL=https://api.firecrawl.dev
```

`GROK_SEARCH_URL` can be a root URL or `/v1` URL. The server automatically calls `/v1/responses`.

## MCP config example for local binary

```json
{
  "mcpServers": {
    "grok-search-rs": {
      "type": "stdio",
      "command": "/absolute/path/to/target/release/grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "your-grok-compatible-key",
        "GROK_SEARCH_URL": "https://api.x.ai",
        "GROK_SEARCH_MODEL": "grok-4-1-fast-reasoning",
        "TAVILY_API_KEY": "your-tavily-key"
      }
    }
  }
}
```

## Tools

| Tool | Purpose |
|---|---|
| `web_search` | Grok search answer with cached sources. |
| `get_sources` | Get full sources for a previous `web_search`. |
| `web_fetch` | Fetch exact page content through Tavily, fallback Firecrawl. |
| `web_map` | Discover URLs through Tavily Map. |
| `doctor` | Live connectivity probe for Grok/Tavily/Firecrawl plus redacted config. |

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

More details:

- [Configuration](docs/CONFIGURATION.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Testing](docs/TESTING.md)

## Star History

<a href="https://www.star-history.com/?repos=Episkey-G%2FGrokSearch-rs&type=Date">
  <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=Episkey-G/GrokSearch-rs&type=Date" />
</a>
