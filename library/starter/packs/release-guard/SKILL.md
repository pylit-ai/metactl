---
name: release-guard
description: Use when preparing a release, handoff, or packaging step and you need a compact readiness gate before the work leaves the branch.
---

# Release Guard

Use this skill to convert vague “looks ready” claims into an explicit release gate.

## Workflow

1. Confirm the release unit: package, binary, tag, or repo handoff.
2. Check the changelog or release notes status.
3. Confirm the versioning or tagging step.
4. Verify the relevant validation commands actually ran.
5. Record the rollback or recovery path before the handoff is called complete.

## Output Format

- Release target
- Validation status
- Missing release blockers
- Ready / not ready decision

## Guardrails

- Do not treat unrun validation as implied green status.
- Do not hand off a release without a rollback note.
- Use `references/checklist.md` for the minimum release checklist.
