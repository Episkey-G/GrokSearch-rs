# GrokSearch-rs

English | [简体中文](#简体中文)

GrokSearch-rs is a Rust MCP server for Grok-backed web search and Tavily-backed source retrieval. It is a Rust rewrite inspired by [GuDaStudio/GrokSearch](https://github.com/GuDaStudio/GrokSearch), with a smaller runtime footprint and explicit provider adapters.

## Overview

GrokSearch-rs keeps the original Grok-with-Tavily product boundary:

```text
Claude / Codex / MCP client
  -> GrokSearch-rs
      -> web_search: Grok provider web search
      -> get_sources: cached search sources
      -> web_fetch: Tavily Extract
      -> web_map: Tavily Map
```

The provider is selected by `GROK_SEARCH_RS_PROVIDER`. Fill the provider you want to use; the server uses that provider directly.

- `anthropic`: calls `/messages` and sends `web_search_20250305`.
- `openai`: calls `/responses` and sends `web_search`; it adds `x_search` only when `GROK_SEARCH_RS_X_SEARCH=true`.

`GROK_SEARCH_RS_WEB_SEARCH` defaults to enabled. `GROK_SEARCH_RS_X_SEARCH` defaults to disabled.

## Features

- Rust stdio MCP server with JSON-RPC 2.0 tool handling.
- Grok primary web search through explicit `anthropic` or `openai` provider adapters.
- Tavily fallback when Grok returns empty content, no verifiable sources, or provider errors.
- Tavily enrichment through `extra_sources` or `GROK_SEARCH_RS_EXTRA_SOURCES`.
- Tavily-owned `web_fetch` and `web_map` tools.
- Source cache keyed by `session_id` for compact `web_search` output and later `get_sources` retrieval.
- Built-in web tool toggle helpers for Claude, Codex, and Gemini project contexts.
- Planning compatibility tools for multi-step search workflows.
- Secret-redacted config diagnostics.

## Installation

Build from source:

```bash
git clone <your-repo-url> grok-search-rs
cd grok-search-rs
cargo build --release
```

The binary is written to:

```text
target/release/grok-search-rs
```

## Configuration

### Anthropic Messages-compatible provider

```bash
GROK_SEARCH_RS_PROVIDER=anthropic
ANTHROPIC_API_KEY=...
ANTHROPIC_API_URL=http://64.186.226.237:8317/v1
ANTHROPIC_MODEL=grok-4-1-fast-reasoning
GROK_SEARCH_RS_WEB_SEARCH=true

TAVILY_API_KEY=...
TAVILY_API_URL=https://api.tavily.com
TAVILY_ENABLED=true
```

### OpenAI Responses-compatible provider

```bash
GROK_SEARCH_RS_PROVIDER=openai
OPENAI_API_KEY=...
OPENAI_API_URL=https://api.x.ai/v1
OPENAI_MODEL=grok-4.3
GROK_SEARCH_RS_WEB_SEARCH=true
GROK_SEARCH_RS_X_SEARCH=true

TAVILY_API_KEY=...
TAVILY_API_URL=https://api.tavily.com
TAVILY_ENABLED=true
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
        "GROK_SEARCH_RS_PROVIDER": "anthropic",
        "ANTHROPIC_API_KEY": "your-provider-key",
        "ANTHROPIC_API_URL": "http://64.186.226.237:8317/v1",
        "ANTHROPIC_MODEL": "grok-4-1-fast-reasoning",
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
| `web_search` | Run Grok-backed web search and cache sources by `session_id`. |
| `get_sources` | Return cached sources for a previous `web_search`. |
| `web_fetch` | Fetch page content through Tavily Extract. |
| `web_map` | Discover site URLs through Tavily Map. |
| `switch_model` | Compatibility helper for changing the configured model at runtime. |
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
  "search_provider": "grok",
  "fallback_used": false,
  "fallback_reason": null
}
```

Use `get_sources` with the `session_id` to retrieve full source metadata.

A Grok result is considered verifiable only when it has non-empty content and native provider sources. When the result is unverifiable, GrokSearch-rs uses Tavily fallback if Tavily is configured.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Architecture notes are in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Test guidance is in [docs/TESTING.md](docs/TESTING.md).

## Troubleshooting

If `sources_count` is `0`:

- Confirm `GROK_SEARCH_RS_WEB_SEARCH` is unset or set to `true`.
- Confirm the selected provider gateway preserves the relevant web search tool.
- Use `get_config_info` to verify the provider, model, Tavily status, and redacted keys.
- Configure `TAVILY_API_KEY` if you expect enrichment or fallback sources.

If `fallback_used=true`, inspect `fallback_reason`:

- `grok_content_empty`: provider returned no answer text.
- `grok_sources_empty`: provider returned answer text but no verifiable native sources.
- `grok_provider_error`: provider request failed and Tavily fallback was used.

## License

MIT. See [LICENSE](LICENSE).

## 简体中文

GrokSearch-rs 是一个 Rust MCP 服务器，用于把 Grok 网络搜索和 Tavily 抓取/映射能力提供给 Claude Code、Codex 等 MCP Client。项目参考了原版 [GuDaStudio/GrokSearch](https://github.com/GuDaStudio/GrokSearch) 的双引擎产品边界，并用 Rust 重构以降低运行时资源占用。

核心约定：

- `web_search` 走 Grok provider 搜索。
- `web_fetch` 走 Tavily Extract。
- `web_map` 走 Tavily Map。
- Grok 无内容、无可信来源或 provider 报错时，`web_search` 使用 Tavily 兜底。
- `GROK_SEARCH_RS_PROVIDER` 填 `anthropic` 就走 `/messages`，填 `openai` 就走 `/responses`。
- `web_search` 默认开启，`x_search` 默认关闭，需要 `GROK_SEARCH_RS_X_SEARCH=true` 才启用。
