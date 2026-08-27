<!-- TRELLIS:START -->
# Trellis Instructions

These instructions are for AI assistants working in this project.

This project is managed by Trellis. The working knowledge you need lives under `.trellis/`:

- `.trellis/workflow.md` — development phases, when to create tasks, skill routing
- `.trellis/spec/` — package- and layer-scoped coding guidelines (read before writing code in a given layer)
- `.trellis/workspace/` — per-developer journals and session traces
- `.trellis/tasks/` — active and archived tasks (PRDs, research, jsonl context)

If a Trellis command is available on your platform (e.g. `/trellis:finish-work`, `/trellis:continue`), prefer it over manual steps. Not every platform exposes every command.

If you're using Codex or another agent-capable tool, additional project-scoped helpers may live in:
- `.agents/skills/` — reusable Trellis skills
- `.codex/agents/` — optional custom subagents

Managed by Trellis. Edits outside this block are preserved; edits inside may be overwritten by a future `trellis update`.

<!-- TRELLIS:END -->

<!-- MATTPOCOCK-GRAFT:START v=1.0.0 -->
## Agent skills

The Trellis phases in `.trellis/workflow.md` are driven by the engineering skills from
[mattpocock/skills](https://github.com/mattpocock/skills), installed under
`.claude/skills/`. Trellis owns the phase gates, task state, and spec injection; those
skills own what happens inside each phase.

### Superseded skills — gated, not just discouraged

These ship with Trellis and still sit in every platform skill root, but this graft
replaced what they do. `install.sh` writes `disable-model-invocation: true` into each
one's `SKILL.md`, so their descriptions are no longer loaded into context and the model
cannot auto-load them. Both stay typable as `/trellis-brainstorm` and `/trellis-check`
if you want one deliberately.

| Skill | Replaced by | Why it needed a mechanism, not a note |
| --- | --- | --- |
| `trellis-brainstorm` | `grill-with-docs` → `to-spec` → `to-tickets` (Phase 1.1) | Its description ends "use when requirements are unclear … or the user describes a new feature", which matches the first prompt of almost every session — while `grill-with-docs` is invisible to the model (see below). Left ungated, it wins that trigger every time. |
| `trellis-check` (the **skill**) | The `trellis-check` **sub-agent**, dispatched twice with `Axis: standards` / `Axis: spec` (Phase 2.2) | Same name, opposite behaviour: the skill self-fixes, the agent is read-only. Its description matches "before committing changes" — exactly when Phase 2.2 wants the Agent form. Verification always uses the Agent form. |

If `trellis update --force` ever overwrites these files, re-run `trellis-graft` to
re-apply the gate.

`trellis-break-loop` is **not** superseded — Phase 3.2 keeps it as a fallback when you want
the 5-dimension root-cause classification without rebuilding a feedback loop.

### Phase 1.1 skills are user-invoked

Four mattpocock skills carry `disable-model-invocation: true` upstream, so their
descriptions never enter context — you cannot see them in your skill list and you cannot
load them:

| Skill | How it runs |
| --- | --- |
| `grill-with-docs` | The user types `/grill-with-docs`. It then loads `grilling` and `domain-modeling`, which you *can* invoke, so the interview proceeds normally from there. |
| `to-spec` | The user types `/to-spec` once the interview has settled. |
| `to-tickets` | The user types `/to-tickets`, only for multi-deliverable scope. |
| `implement` | Never invoked directly — Phase 2.1 routes through the `trellis-implement` sub-agent, which drives `tdd` itself, so implementation work never lands in the main session's context. |

When you reach Phase 1.1, **ask the user to type `/grill-with-docs`** and wait. Do not
improvise an interview from the workflow.md summary: the rules that make grilling work
(research the facts before asking, decisions belong to the user, resolve every branch of
the design tree) live in the skill body you cannot read. A simulated interview looks like
progress and produces a `prd.md` that never got stress-tested.

The same applies at every stage boundary — the chain does not advance on its own.
`grill-with-docs` is one line that delegates to `/grilling` and `/domain-modeling`; it
never hands off to `to-spec`. So when the interview has resolved every branch, **ask the
user to type `/to-spec`** instead of writing `prd.md` yourself, and ask for `/to-tickets`
only when the scope has several independently verifiable deliverables.

These four live in `~/.claude/skills/`, which is user-global — this graft installs
per-repo and does not modify them.

### Issue tracker

**This section is the configuration** that `to-spec`, `to-tickets`, and `research` mean when
they say the tracker and label vocabulary "should have been provided to you". It is provided,
here. Do **not** run `/setup-matt-pocock-skills` — it rewrites this block in place, and with
no git remote its default answer is a `.scratch/` tracker, which is exactly what this graft
replaces.

Those skills carry unconditional publish instructions written for GitHub. Translate on sight;
do not re-derive the mapping each time, and do not report the mismatch as a finding:

| The skill's words | What to actually do |
| --- | --- |
| "publish the spec to the project issue tracker" | Write the active task's `prd.md`. `python3 ./.trellis/scripts/task.py current --source` gives the path. Never a file outside the task tree. |
| "apply the `ready-for-agent` triage label" | Nothing to apply — there are no labels here. Lifecycle state is `task.json.status`, and what actually marks a task agent-ready is `task.py start` at step 1.4, after the artifact review. Not at spec-writing time. |
| "publish the tickets to the configured tracker" | `task.py create "<title>" --slug <name> --parent <parent-dir>`, one child task per slice, blockers first. Not `.scratch/<feature>/issues/NN-slug.md`, not `gh issue create`. |
| "blocking edges" / "Blocked by" | Write them into each child's own `prd.md`. The tree carries parent/child, but tree position never implies a blocking edge. |
| "fetch the relevant ticket" | `task.py current --source`, then read that directory's `prd.md`. |

Full mapping, including the reasoning: `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Triage labels

None. The `triage` skill is not installed and would be redundant if it were: it is a state
machine for issues **other people file** (`needs-triage` → `needs-info` → `ready-for-agent`
→ `ready-for-human` → `wontfix`). Trellis tasks are ones you grilled into existence, so the
first two states cannot occur, `ready-for-agent` is `task.py start`, and `wontfix` is
`task.py archive`. Lifecycle state lives in `task.json.status`.
<!-- MATTPOCOCK-GRAFT:END -->
