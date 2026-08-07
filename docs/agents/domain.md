# Domain Docs

How the engineering skills should consume this repo's domain documentation when
exploring the codebase. This repo is **single-context**.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — the glossary / ubiquitous language
- **`docs/adr/`** — read ADRs that touch the area you're about to work in

If either doesn't exist, **proceed silently**. Don't flag their absence and don't
suggest creating them upfront. The `domain-modeling` skill (reached via
`grill-with-docs` at Trellis Phase 1.1) creates them lazily, when terms or decisions
actually get resolved.

## File structure

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-<decision>.md
│   └── 0002-<decision>.md
└── src/
```

## Relationship to `.trellis/spec/`

They are different things and neither replaces the other:

- **`CONTEXT.md` + `docs/adr/`** — *what the words mean* and *why a hard-to-reverse
  decision went the way it did*. Maintained by `domain-modeling`, consumed by every
  skill that names a domain concept.
- **`.trellis/spec/`** — *how code in a given package and layer must be written*.
  Maintained at Trellis Phase 3.3, injected into sub-agents by the Trellis hooks.

Rule of thumb: a naming or modelling decision goes in `CONTEXT.md` / an ADR; a coding
convention or a pitfall to avoid goes in `.trellis/spec/`.

## Use the glossary's vocabulary

When your output names a domain concept (a task title, a refactor proposal, a
hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to
synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal — either you're
inventing language the project doesn't use (reconsider), or there's a real gap (note
it for `domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently
overriding:

> _Contradicts ADR-0007 — but worth reopening because…_
