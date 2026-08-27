# Search quality safety baseline

`grok-search-rs` uses a staged quality-safety rollout. The current stage is
**shadow observation only**: it evaluates the final user-visible result, emits
an optional aggregate diagnostic event, and returns the original result
unchanged.

It does **not**:

- add a planner/model call;
- issue a second search;
- rewrite a query;
- change source ordering or deduplication;
- add fields to the MCP response.

## Enable shadow observation

Shadow observation is disabled by default. Enable it for an operator-controlled
stdio or HTTP process with:

```bash
GROK_SEARCH_QUALITY_GATE_SHADOW=true
```

The equivalent global TOML setting is:

```toml
quality_gate_shadow_enabled = true
```

Each successful `web_search` writes one compact JSON object to stderr. stdout
remains reserved for MCP JSON-RPC. Observation/serialization errors are ignored
and cannot turn a successful search into an error.

The event contains only aggregate data:

- random search `session_id` and elapsed milliseconds;
- answer character count, provider, fallback/truncation state;
- cached and user-visible source counts;
- counts of valid HTTP URLs, unique hosts/providers, and available metadata;
- stable hard-failure and advisory reason codes;
- hypothetical `would_retry` for later threshold analysis.

Raw query text, answer text, URLs, source content, titles, and domain-filter
values are deliberately excluded. The event is diagnostic evidence, not a
quality score and not a promise that a retry would improve the answer.

## Conservative URL identity

Shadow analysis uses `url::Url` only to recognize equivalence guaranteed by the
URL Standard, such as host/scheme casing, a default port, a root slash, IDNA,
and dot-segment normalization. It does not rewrite returned URLs and does not
currently change source merging.

The identity deliberately keeps these differences distinct:

- `http` versus `https`;
- apex versus `www`/other subdomains;
- trailing slash and path case;
- query parameters, their order, and tracking parameters;
- fragments;
- userinfo and non-default ports.

This conservative boundary avoids silently losing long-tail or SPA/hash-route
sources before the evaluation set demonstrates that broader canonicalization is
safe.

## Offline baseline

Run the deterministic, provider-free baseline with:

```bash
cargo test --locked --test quality_baseline
```

The versioned fixture at `tests/fixtures/quality/cases.json` covers:

1. primary/official-source placement;
2. a fixed recency window;
3. two-entity comparison coverage;
4. Chinese query and UTF-8 preservation;
5. exact duplicate removal with first-source metadata preserved.

The test injects scripted AI and source providers through the production
`SearchService` orchestration. It verifies query/filter propagation, prompt
requirements, merge behavior, source validity, required host coverage, and
deterministic output without calling live services.

This baseline proves pipeline contracts only. It cannot prove real-model factual
correctness or current web recall; those require a separate, explicitly invoked
live evaluation with frozen questions and human-reviewed ground truth.

## Gate for a future focused retry

Automatic retry remains disabled until shadow data and live evaluation show a
net quality gain. A later one-retry implementation must satisfy all of the
following:

- keep the current fast path unchanged for passing results;
- preserve the original query and structured constraints;
- allow at most one additional focused search within the same deadline budget;
- derive hard domain filters only from user input, verified mappings, or actual
  first-round sources;
- retain the first result and accept merged evidence only when measured quality
  improves;
- fall back to the first result on timeout, provider error, no new unique source,
  or conflicting unverifiable evidence;
- show no regression in answer correctness, citation support, primary-source
  hit rate, constraint preservation, and normal-query latency.

Full query graphs and multi-round research belong in a separate
`deep_research` capability rather than the default `web_search` path.
