---
name: migration-guard
description: Use when a task touches migrations, schema history, or destructive database changes and explicit approval boundaries need to stay visible.
---

# Migration Guard

Migration edits are high-risk because small text changes can have irreversible production impact.

## Workflow

1. Confirm whether the task actually requires editing an existing migration.
2. Prefer additive or forward-only changes before considering a rewrite of applied history.
3. If the change touches historical migrations, dropping data, or transaction semantics, stop and surface the approval boundary.
4. Summarize the blast radius, rollback path, and whether prior versions may already be applied.
5. Only proceed after explicit approval.

## Output Format

- Risk summary
- Whether approval is required
- Recommended safer alternative if one exists

## Guardrails

- Do not rewrite migration history silently.
- Do not treat schema changes as ordinary refactors.
- Use `ESCALATION.md` when the task crosses the explicit approval boundary.
