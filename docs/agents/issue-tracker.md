# Issue tracker: Trellis tasks

Issues, specs, and tickets for this repo live in the **Trellis task tree** under
`.trellis/tasks/`, not on GitHub Issues and not under `.scratch/`.

This file is the seam between the engineering skills (`to-spec`, `to-tickets`,
`implement`, `research`) and Trellis. Where a skill says "the issue tracker",
read the mapping below.

## Mapping

| Skill vocabulary | Trellis equivalent |
| --- | --- |
| The spec | The active task's `prd.md` (plus `design.md` / `implement.md` for complex tasks) |
| A ticket | A Trellis child task directory under `.trellis/tasks/` |
| Publishing to the tracker | `task.py create` — never a hand-written file outside the task tree |
| Fetching the relevant ticket | `task.py current --source`, then read that directory's `prd.md` |
| Triage status | `task.json.status` (`planning` / `in_progress` / `completed`) |

## When a skill says "publish the spec to the issue tracker"

Write it into the **active task's `prd.md`**. Do not create a GitHub issue and do not
write to `.scratch/`. Keep `prd.md` to requirements and acceptance criteria — technical
design goes in `design.md`, the ordered execution checklist in `implement.md`.

## When a skill says "publish the tickets to the tracker"

Create one Trellis child task per ticket, in dependency order (blockers first):

```bash
python3 ./.trellis/scripts/task.py create "<ticket title>" --slug <name> --parent <parent-task-dir>
```

Then write each child's `prd.md` with the ticket body: what it delivers end to end,
its acceptance criteria, and a `## Blocked by` section.

**Blocking edges are text, not structure.** Trellis parent/child is a grouping
mechanism, not a dependency system — the tree carries no ordering. Every blocking
edge must be written explicitly into the blocked child's `prd.md`:

```markdown
## Blocked by

- `08-15-schema-migration` — needs the new column to exist
```

A child with no `## Blocked by` section can start immediately.

**Working the frontier**: any child task whose listed blockers are all `completed`
(or archived) is startable. Start it with `task.py start <task-dir>`; that flips it to
`in_progress` and switches the per-turn breadcrumb to the execution phase.

## Triage labels

Not used. The `triage` skill is not installed in this repo, and Trellis carries
lifecycle state in `task.json.status` instead of label strings. Ignore any instruction
to apply a `ready-for-agent` label — starting the task is the equivalent signal.

## Research output

`research` writes its cited Markdown into the **active task's** `research/` directory
(`<task-dir>/research/<topic>.md`), so it travels with the task and lands in the
archive with it. Not into a repo-wide research folder.
