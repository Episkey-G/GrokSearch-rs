# Testing

## Local Verification

Run the full local verification suite:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
cargo test --all-features --locked
node scripts/check-public-contract.mjs
```

## Targeted Tests

| Area | Command |
|---|---|
| Config parsing | `cargo test --test config` |
| CLI setup and doctor | `cargo test --test cli` |
| Grok Responses payload and response adapters | `cargo test --test adapter_grok_responses` |
| Search orchestration | `cargo test --test service_contract` |
| Offline search-quality baseline | `cargo test --locked --test quality_baseline` |
| Tavily parsing | `cargo test --test tavily_parse` |
| Source merge behavior | `cargo test --test source_merge` |
| Logging | `cargo test --test logging` |

## Live Smoke Testing

Live provider tests require real API keys and should not be committed as logs.
The default test suite and CI never run them. `grok-search-rs doctor` and the
setup wizard's optional final probe do contact configured providers and may use
provider quota.

The offline quality baseline uses scripted providers and never contacts the
network. It validates deterministic orchestration and source-quality contracts;
it does not claim to measure live-model factual accuracy. See
[SEARCH_QUALITY.md](SEARCH_QUALITY.md) for its fixed scenarios and limitations.

Recommended smoke matrix:

1. `GROK_SEARCH_URL=https://api.x.ai` or another compatible gateway root URL.
2. `GROK_SEARCH_X_SEARCH=false` for baseline Responses `web_search` only.
3. `GROK_SEARCH_X_SEARCH=true` only when the gateway is known to preserve `x_search`.
4. Tavily fallback by forcing an empty or source-less Grok response in tests.
5. `web_fetch` against a stable public URL, first with Tavily, then with Firecrawl fallback.
6. `web_map` with a small `max_results` value.

Store live logs under `logs/`; the directory is ignored by git.
