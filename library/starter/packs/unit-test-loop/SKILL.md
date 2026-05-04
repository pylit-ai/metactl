---
name: unit-test-loop
description: Use when code changes need a focused verification loop and the fastest trustworthy signal is a narrow, reproducible test run.
---

# Unit Test Loop

Use the smallest test command that exercises the edited boundary, then widen only when the first signal says you need to.

## Workflow

1. Identify the narrowest test target that covers the changed behavior.
2. Run that targeted command before or immediately after the edit.
3. If it fails, report the failure clearly before widening scope.
4. Only widen from test name to file, package, or broader suite when the narrow loop is insufficient.
5. Record the exact command so another engineer can reproduce it.

## Output Format

- Exact test command run
- Result: pass, fail, or blocked
- Why a broader command was or was not needed

## Guardrails

- Do not jump to the full suite first unless the boundary is genuinely unclear.
- Do not claim verification without naming the command that produced the signal.
- Use `SCOPE.md` and `commands/run-targeted-tests.md` when selecting or sharing the command.
