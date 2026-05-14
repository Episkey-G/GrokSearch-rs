# Testing

## Local Verification

Run the full local verification suite:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Targeted Tests

| Area | Command |
|---|---|
| Config parsing | `cargo test --test config` |
| Provider payload adapters | `cargo test --test adapter_anthropic` |
| Search orchestration | `cargo test --test service_contract` |
| Tavily parsing | `cargo test --test tavily_parse` |
| Built-in tool toggles | `cargo test --test toggle_builtin_tools` |
| Planning tools | `cargo test --test planning` |
| Source merge behavior | `cargo test --test source_merge` |

## Live Smoke Testing

Live provider tests require real API keys and should not be committed as logs.

Recommended smoke matrix:

1. `GROK_SEARCH_RS_PROVIDER=anthropic` with `/messages` and `GROK_SEARCH_RS_WEB_SEARCH=true`.
2. `GROK_SEARCH_RS_PROVIDER=openai` with `/responses` and `GROK_SEARCH_RS_X_SEARCH=false`.
3. `GROK_SEARCH_RS_PROVIDER=openai` with `/responses` and `GROK_SEARCH_RS_X_SEARCH=true` only when the gateway is known to preserve `x_search`.
4. Tavily fallback by forcing an empty or source-less AI provider response.
5. `web_fetch` against a stable public URL.
6. `web_map` with a small `max_results` value.

Store live logs under `logs/`; the directory is ignored by git.
