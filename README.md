# GrokSearch-rs

![GrokSearch-rs product banner](assets/groksearch-rs-banner.png)

English | [简体中文](#简体中文)

GrokSearch-rs is a Rust MCP server for Grok Responses web search, Tavily source retrieval, and Firecrawl fetch fallback. It is a Rust rewrite inspired by [GuDaStudio/GrokSearch](https://github.com/GuDaStudio/GrokSearch), with a smaller runtime footprint and a stricter `/v1/responses` provider contract.

## Overview

GrokSearch-rs keeps the Grok-with-source-tools product boundary:

```text
Claude / Codex / MCP client
  -> GrokSearch-rs
      -> web_search: Grok Responses /v1/responses with web_search enabled
      -> get_sources: cached search sources
      -> web_fetch: Tavily Extract, Firecrawl scrape fallback
      -> web_map: Tavily Map
```

There is one AI entrypoint: `GROK_SEARCH_URL` plus `/v1/responses`. You can provide a root URL such as `https://api.x.ai`, a `/v1` base URL, or an endpoint-like URL; GrokSearch-rs normalizes it to a `/v1` base and calls `/responses` automatically.

`web_search` is enabled by default. `x_search` is disabled by default and is only sent when `GROK_SEARCH_X_SEARCH=true`.

## Features

- Rust stdio MCP server with JSON-RPC 2.0 tool handling.
- Grok primary web search through OpenAI/xAI-compatible Responses payloads.
- Tavily fallback when Grok returns empty content, no verifiable sources, or provider errors.
- Tavily enrichment through `extra_sources` or `GROK_SEARCH_EXTRA_SOURCES`.
- Firecrawl supplemental source fallback and `web_fetch` fallback.
- Tavily-owned `web_fetch` primary extraction and `web_map` site discovery.
- Source cache keyed by `session_id` for compact `web_search` output and later `get_sources` retrieval.
- Built-in web tool toggle helpers for Claude, Codex, and Gemini project contexts.
- Planning compatibility tools for multi-step search workflows.
- Secret-redacted config diagnostics.

## Installation

Build from source:

```bash
git clone https://github.com/Episkey-G/GrokSearch-rs.git grok-search-rs
cd grok-search-rs
cargo build --release
```

The binary is written to:

```text
target/release/grok-search-rs
```

## Configuration

```bash
GROK_SEARCH_API_KEY=...
GROK_SEARCH_URL=https://api.x.ai
GROK_SEARCH_MODEL=grok-4.3
GROK_SEARCH_WEB_SEARCH=true
GROK_SEARCH_X_SEARCH=false

TAVILY_API_KEY=...
TAVILY_API_URL=https://api.tavily.com

# Optional Firecrawl fallback.
FIRECRAWL_API_KEY=...
FIRECRAWL_API_URL=https://api.firecrawl.dev

GROK_SEARCH_EXTRA_SOURCES=0
GROK_SEARCH_FALLBACK_SOURCES=5
GROK_SEARCH_CACHE_SIZE=256
GROK_SEARCH_TIMEOUT_SECONDS=60
```

More configuration details are in [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

## MCP Registration

Claude Code example:

```json
{
  "mcpServers": {
    "grok-search-rs": {
      "type": "stdio",
      "command": "/absolute/path/to/target/release/grok-search-rs",
      "env": {
        "GROK_SEARCH_API_KEY": "your-provider-key",
        "GROK_SEARCH_URL": "https://api.x.ai",
        "GROK_SEARCH_MODEL": "grok-4.3",
        "GROK_SEARCH_X_SEARCH": "false",
        "TAVILY_API_KEY": "your-tavily-key"
      }
    }
  }
}
```

## MCP Tools

| Tool | Purpose |
|---|---|
| `health` | Return redacted runtime configuration. |
| `get_config_info` | Return structured redacted runtime configuration. |
| `web_search` | Run Grok Responses web search and cache sources by `session_id`. |
| `get_sources` | Return cached sources for a previous `web_search`. |
| `web_fetch` | Fetch page content through Tavily Extract with Firecrawl fallback. |
| `web_map` | Discover site URLs through Tavily Map. |
| `switch_model` | Compatibility helper for changing the configured model per call. |
| `toggle_builtin_tools` | Disable or enable built-in web tools for supported agent hosts. |
| `plan_search` | One-shot search planning helper. |
| `plan_intent` | Phased planning: capture intent. |
| `plan_complexity` | Phased planning: capture complexity. |
| `plan_sub_query` | Phased planning: define sub-query. |
| `plan_search_term` | Phased planning: define search term. |
| `plan_tool_mapping` | Phased planning: map sub-query to tool. |
| `plan_execution` | Phased planning: define execution order. |

## Search Strategy

`web_search` returns compact output:

```json
{
  "session_id": "...",
  "content": "...",
  "sources_count": 3,
  "search_provider": "grok_responses",
  "fallback_used": false,
  "fallback_reason": null
}
```

Use `get_sources` with the `session_id` to retrieve full source metadata.

Recommended agent flow:

```text
web_search for concise sourced summary
get_sources before source-specific claims, citation lists, or follow-up fetches
web_fetch for exact page evidence, quotes, technical details, or when the summary is insufficient
```

A Grok result is considered verifiable only when it has non-empty content and native Responses sources. When the result is unverifiable, GrokSearch-rs uses Tavily first and Firecrawl second for fallback sources when configured.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Architecture notes are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Test guidance is in [docs/TESTING.md](docs/TESTING.md).

## Troubleshooting

If `sources_count` is `0`:

- Confirm `GROK_SEARCH_WEB_SEARCH` is unset or set to `true`.
- Confirm the configured gateway preserves Responses `web_search` tool calls.
- Use `get_config_info` to verify the model, Tavily status, Firecrawl status, and redacted keys.
- Configure `TAVILY_API_KEY` or `FIRECRAWL_API_KEY` if you expect enrichment or fallback sources.

If `fallback_used=true`, inspect `fallback_reason`:

- `grok_content_empty`: provider returned no answer text.
- `grok_sources_empty`: provider returned answer text but no verifiable native sources.
- `grok_provider_error`: provider request failed and source fallback was used.

## License

MIT. See [LICENSE](LICENSE).

## 简体中文

GrokSearch-rs 是一个 Rust MCP 服务器，用于把 Grok Responses 网络搜索、Tavily 来源检索/网页抓取，以及 Firecrawl 抓取兜底提供给 Claude Code、Codex 等 MCP Client。项目参考了原版 [GuDaStudio/GrokSearch](https://github.com/GuDaStudio/GrokSearch) 的产品边界，并用 Rust 重构以降低运行时资源占用。

核心约定：

- `web_search` 只走 Grok Responses `/v1/responses`。
- `web_search` 默认发送 `web_search`；只有 `GROK_SEARCH_X_SEARCH=true` 时才发送 `x_search`。
- `web_fetch` 优先走 Tavily Extract，失败后可走 Firecrawl scrape 兜底。
- `web_map` 走 Tavily Map。
- Grok 无内容、无可信来源或 provider 报错时，`web_search` 使用 Tavily/Firecrawl 来源兜底。
- `GROK_SEARCH_URL` 可填写根地址或 `/v1` 地址，程序会自动归一化并调用 `/v1/responses`。

## Star History

<a href="https://www.star-history.com/?repos=Episkey-G%2FGrokSearch-rs&type=Date">
  <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=Episkey-G/GrokSearch-rs&type=Date" />
</a>
