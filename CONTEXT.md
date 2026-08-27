# GrokSearch-rs

An MCP server that answers web queries through an AI upstream, and backs every
answer with independently retrieved evidence. This glossary fixes the words for
the two very different things that retrieve that evidence — a distinction users
have already gotten wrong in bug reports because our own diagnostics blurred it.

## Language

### Evidence retrieval

**Source provider**:
An external, API-key-gated service that can search the web and fetch page text:
Tavily, Exa, TinyFish, Firecrawl. Without a key, a source provider does not
exist at runtime.
_Avoid_: specialist, source plugin, search backend

**Specialist extractor**:
A key-free, site-specific parser that renders one family of URLs into structured
Markdown: GitHub issues/PRs/releases, StackExchange, arXiv, Wikipedia. It is
never configured with a key and is never part of the source chain.
_Avoid_: specialist provider, specialist key, parser provider

**Source chain**:
The ordered list of configured source providers. The first one that returns
results wins; the order is canonical unless `GROK_SEARCH_SOURCE_PROVIDERS`
overrides it.
_Avoid_: provider list, fallback chain

**Key ring**:
The rotating set of API keys configured for a single upstream, expressed as one
delimited string. Rotation advances round-robin and fails over only on
key-scoped errors.
_Avoid_: key pool, key list

### Search outcomes

**Enrichment**:
Attaching page text to sources that a *successful* AI answer already cited or
that the source chain supplied alongside it.
_Avoid_: inline fetch, content hydration

**Source fallback**:
The degraded path taken when the AI upstream cannot verify itself — no answer,
or an answer with no citations. The source chain's results replace the answer;
they never decorate it.
_Avoid_: fallback sources (that name belongs to the config knob), backup search

**Request deadline**:
The single wall-clock budget covering one tool call end to end — AI upstream,
key rotation, retries, source fan-out, and enrichment together. Nothing inside
a call may grant itself budget beyond it. See ADR-0001.
_Avoid_: timeout (that names the config knob), per-attempt budget
