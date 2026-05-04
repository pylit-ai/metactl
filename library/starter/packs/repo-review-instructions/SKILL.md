---
name: repo-review-instructions
description: Use when reviewing code or proposed changes and the task needs a correctness-first, risk-first review instead of speculative rewriting.
---

# Repo Review Instructions

Review for correctness first. Find concrete bugs, regressions, and missing verification before offering polish.

## Workflow

1. Inspect the changed behavior and likely blast radius.
2. Look for correctness issues, safety regressions, contract drift, and missing tests.
3. Escalate uncertainty explicitly instead of guessing.
4. Keep findings concrete enough that another engineer can verify them quickly.

## Output Format

- Findings ordered by severity
- Open questions or assumptions
- Brief summary only after the findings

## Guardrails

- Prefer evidence-backed findings over style commentary.
- Do not speculate about bugs you cannot tie to code paths or behavior.
- If there are no findings, say that explicitly and mention residual test gaps.
