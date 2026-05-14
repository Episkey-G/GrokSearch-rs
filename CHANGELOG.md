# Changelog

All notable changes to GrokSearch-rs are documented here.

## 0.1.0 - 2026-05-14

### Added

- Rust MCP stdio server for Grok-backed web search and Tavily-backed source retrieval.
- `anthropic` provider using `/messages` and `web_search_20250305`.
- `openai` provider using `/responses` with `web_search` and optional `x_search`.
- Tavily search fallback when Grok returns empty content, no verifiable sources, or provider errors.
- Tavily Extract-backed `web_fetch` and Tavily Map-backed `web_map`.
- Source cache keyed by `session_id` and `get_sources` retrieval.
- Planning compatibility tools and built-in tool toggle support for Claude, Codex, and Gemini contexts.
- Regression coverage for provider payload shape, fallback behavior, Tavily parsing, source merging, planning, logging, and toggle aliases.

### Changed

- Repository documentation now separates user configuration, architecture, and verification guidance.
- `GROK_SEARCH_RS_WEB_SEARCH` defaults to enabled.
- `GROK_SEARCH_RS_X_SEARCH` defaults to disabled and must be explicitly enabled.

### Fixed

- Prevented the original GrokSearch issue #41 class of failure by ensuring provider payloads include the intended web search tool for supported protocols.
- Treated empty or source-less Grok responses as unverifiable and routed them to Tavily fallback.
