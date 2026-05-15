# Contributing

Thanks for improving GrokSearch-rs. This repository is intentionally small, so changes should keep the product easy to audit and operate.

## Development Setup

```bash
cargo build
cargo test
```

Use Rust 2021 and keep code formatted with `cargo fmt`.

## Change Guidelines

- Keep MCP tool contracts stable unless the README and tests are updated in the same change.
- Do not log or commit API keys, bearer tokens, cookies, or raw request headers containing secrets.
- Keep provider behavior explicit: `anthropic` means `/messages`; `openai` means `/responses`.
- Preserve Tavily's role split: `web_search` uses Tavily for enrichment/fallback, while `web_fetch` and `web_map` are Tavily-owned capabilities.
- Add or update tests for config parsing, provider payloads, source provenance, and fallback behavior when changing search flow.

## Verification Checklist

Before committing a code change, run:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

For documentation-only changes, at minimum inspect changed Markdown for stale command names and secret-like values.

## Releasing

See [RELEASING.md](./RELEASING.md) for the version-bump and tag workflow.
