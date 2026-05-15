# Changelog

All notable changes to GrokSearch-rs are documented here.

## 0.1.5 - 2026-05-15

### Removed

- Planning compatibility tools (`plan_intent`, `plan_search`, `plan_search_term`, `plan_sub_query`, `plan_tool_mapping`, `plan_execution`, `plan_complexity`) and their tests.
- Built-in tool toggle support (`toggle_builtin_tools`) and its test.
- Auxiliary tools `health`, `get_config_info`, `switch_model` from the MCP surface.

### Changed

- Reduced MCP surface to 5 tools: `web_search`, `get_sources`, `web_fetch`, `web_map`, `doctor`.
- Replaced ad-hoc health/config probes with a single `doctor` diagnostic that performs live connectivity checks against Grok, Tavily, and Firecrawl and returns masked configuration.
- Tightened provider modules (`grok`, `tavily`, `firecrawl`) and simplified `SearchService` source caching.

### Added

- Tag-driven release pipeline: pushing `vX.Y.Z` builds binaries, publishes 6 npm packages, and syncs `Cargo.toml` / `Cargo.lock` / all `package.json` files back to `main` automatically.
- Manual fallback `scripts/bump-version.sh` and `Bump Version` GitHub Actions workflow.

## 0.1.4 - 2026-05-15

### Fixed

- Ignored JSON-RPC notifications such as `notifications/initialized` instead of emitting `id: null` error responses during MCP startup.
- Added MCP `ping` request support.

## 0.1.3 - 2026-05-15

### Fixed

- Aligned the npm launcher with `ace-tool-rs` by resolving the installed platform package directly and removing runtime GitHub release download fallback from MCP startup.

## 0.1.0 - 2026-05-14

### Added

- Rust MCP stdio server for Grok Responses-backed web search, Tavily source retrieval, and Firecrawl fallback.
- Single Grok Responses provider using `/v1/responses` with `web_search` enabled by default and optional `x_search`.
- `GROK_SEARCH_URL` normalization from root URL, `/v1` base URL, or endpoint-like URL to a `/v1` base.
- Tavily search fallback when Grok returns empty content, no verifiable sources, or provider errors.
- Tavily Extract-backed `web_fetch` and Tavily Map-backed `web_map`.
- Firecrawl-backed `web_fetch` fallback and supplemental source fallback.
- Source cache keyed by `session_id` and `get_sources` retrieval.
- Planning compatibility tools and built-in tool toggle support for Claude, Codex, and Gemini contexts.
- Regression coverage for provider payload shape, fallback behavior, Tavily parsing, source merging, planning, logging, and toggle aliases.

### Changed

- Public AI configuration now uses `GROK_SEARCH_API_KEY`, `GROK_SEARCH_URL`, and `GROK_SEARCH_MODEL`.
- `GROK_SEARCH_WEB_SEARCH` defaults to enabled.
- `GROK_SEARCH_X_SEARCH` defaults to disabled and must be explicitly enabled.

### Fixed

- Prevented the original GrokSearch issue #41 class of failure by ensuring Responses payloads include the intended web search tool.
- Treated empty or source-less Grok responses as unverifiable and routed them to source fallback.
