# One request deadline per tool call, and nothing may outlive it

Adding multi-key rotation and 429/503 retry to the AI upstream (issue #14)
creates a way for a single `web_search` to spend far more wall-clock than the
configured timeout: the AI call is bounded only by the reqwest client timeout,
so N keys x M retries would multiply it. We decided that one tool call gets one
request deadline, and every retry, key rotation, source fan-out, and enrichment
step draws from that same budget — a step that finds the budget spent gives up
immediately rather than starting.

This matters more than it looks, because the failure it prevents is invisible.
An MCP client's own tool-call timeout is the same order of magnitude as ours
(~60s). If the server outruns the client, the client reports nothing at all —
not our carefully classified error. A structured error is only worth writing if
we are still allowed to speak when we write it. Issue #39 is exactly this
failure observed from the client side.

## Considered options

- **Per-attempt budgets** (each retry gets a fresh client timeout). Rejected:
  worst case grows with key count, and the caller has no knob that bounds it.
- **Cap retries at 1 instead of bounding time.** Rejected: it bounds the count,
  not the duration; one slow attempt plus one slow retry still overruns.
- **Let retries overrun and rely on the client to wait.** Rejected: clients do
  not wait, and the overrun is precisely what makes failures silent.

## Consequences

- The default `GROK_SEARCH_TIMEOUT_SECONDS` drops from 60 to 45, so the server
  reliably reports its own failure before a 60s-class client gives up.
- Retry backoff must check remaining budget before sleeping. A `Retry-After`
  longer than what is left is not honored — the attempt is abandoned instead.
- Every future upstream that wants retry or rotation inherits this rule. If one
  needs its own budget, that is a change to this decision, not an exception to
  it.
