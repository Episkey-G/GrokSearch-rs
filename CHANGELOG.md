# Changelog

All notable changes to GrokSearch-rs are documented here.

## 0.1.8 - 2026-05-15

### Performance

- `web_search` 改为投机并发：`tokio::join!` 同时发起 Grok 与 Tavily 检索，总延迟由 `sum(Grok, Tavily)` 降为 `≈ max(Grok, Tavily)`；通过 `count = max(extra_sources, fallback_sources)` 一次取够、按路径裁剪复用，**保持"每次 web_search 仅一次源 provider 调用"契约**。
- 三家 provider（Grok / Tavily / Firecrawl）共享单个 `reqwest::Client`，启用 `gzip`、`pool_idle_timeout=90s`、`tcp_keepalive=60s`、`tcp_nodelay`；TLS 会话与连接池跨 host 复用。
- HTTP 响应解析切 `bytes()` + `serde_json::from_slice`，省去一次完整 UTF-8 校验扫描。
- `apply_fetch_limit` 改单次 `char_indices` 截断，UTF-8 文本由三遍扫描压到一遍。
- `Source.provider` 字段由 `String` 切 `Cow<'static, str>`：所有内部标签都是 `'static`，逐源省去一次堆分配。
- `SourceCache` 内部存 `Arc<Vec<Source>>`：cache get/set 在 mutex 内仅做引用计数，临界区由 O(N) 深拷降为 O(1)。
- Tavily search 请求体改 serde 派生结构体：消除 `json! + as_object_mut + insert` 多次临时 String 分配。
- `session_id` 编码改栈缓冲（`uuid::fmt::Simple::encode_lower` 写 `[u8; 32]`），省两次 String 分配。

### Changed

- tokio runtime flavor 由 `multi_thread` 切 `current_thread`：MCP stdio 服务本就单流，去掉 N 个 worker 线程降低稳态 RSS（预期 0.3~0.8 MB）。
- `[profile.release]` 启用 `panic = "abort"`：移除 unwind 表，release 二进制由 3.0 MB 降至 **2.5 MB（−16.6%）**。
- `reqwest` 启用 `gzip` feature。

### Internal

- 新增 `GrokResponsesProvider::with_client` / `TavilyProvider::with_client` / `FirecrawlProvider::with_client` 构造路径，旧 `new(.., timeout)` 签名保留以兼容下游集成。
- 新增 `RawSourceOrigin` 枚举与 `enrichment_label` / `fallback_label` 自由函数，把"取源"与"打路径标签"解耦。
- 测试新增 3 条契约：`get_sources_returns_same_payload_repeatedly`、`web_search_speculation_serves_enrichment_with_one_source_call`、`source_provider_field_accepts_static_str_via_cow`。

### Verified

- `cargo test --all`：34 passed / 0 failed
- `cargo clippy --all-targets -- -D warnings`：零警告
- MCP stdio 烟测：`initialize` + `tools/list` 协议握手通过，五工具齐全
- 所有公共 MCP tool 输入 / 输出 schema **零变更**

## 0.1.7 - 2026-05-15

### Added

- `web_search` 新增 `recency_days` / `include_domains` / `exclude_domains` 输入参数：Tavily 端真过滤（`days` + `topic=news` 及 include/exclude 域名），Grok 端以软提示形式注入 prompt。
- `web_fetch` 新增 `max_chars` 输入参数与 `GROK_SEARCH_FETCH_MAX_CHARS` 环境兜底；返回结构扩展为 `{url, content, original_length, truncated}`，便于 LLM 感知截断。

### Changed

- `web_search` 输出回炉：撤掉懒加载契约与 `sources_preview` 字段，改为常驻 `sources: [...]`——成功路径返回 Grok 原生 + Tavily 补强 merge 后的完整列表；fallback 路径返回 Tavily 兜底的完整列表。每条含 `{url, provider, title?, description?, published_date?}`。`session_id` 与 `get_sources` 保留作缓存回查入口，但不再是获取首次响应来源的必经路径。
- `GROK_SEARCH_EXTRA_SOURCES` 默认值由 `0` 调整为 `3`，使开放检索默认即享 Tavily 补强；如需关闭显式设 `0`。
- `SourceProvider::search_sources` trait 签名扩展接收 `&SearchFilters`，Tavily 透传，Firecrawl 忽略（无对应能力）。

## 0.1.6 - 2026-05-15

### Fixed

- `doctor` 的 Grok 探针现在携带 `web_search` tool intent，避免上游误判为 parse error 导致 `reachable=false` 与实际可用状态不符。

### Changed

- 默认 `GROK_SEARCH_MODEL` 由 `grok-4.3` 调整为 `grok-4-1-fast-reasoning`（同步 README、`.env.example`、docs/CONFIGURATION.md、tests/config.rs）。
- `web_map` 输出裁剪为仅 `{url, provider}`，剥离对地图发现场景无用的 `title` / `description` / `published_date`，减小响应体。
- 抽出 `src/providers/http.rs` 公共 `build_client` 与 `post_json`，三个 provider（Grok / Tavily / Firecrawl）共享同一份 reqwest client 构造与 HTTP 错误归类逻辑。
- 合并测试用 4 个 `fake_with_*` 工厂方法为 `fake_with_sources` + `fake_custom`，净减约 70 行测试样板。
- README Tools 表与 docs/TESTING.md 清理 0.1.5 已下线的工具与测试条目，与当前 5 件 MCP 工具表面对齐。

### Removed

- 本地 `GrokSearch-rs-rebuild-plan.md` 历史规划稿（原本即在 `.gitignore` 内）。

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
