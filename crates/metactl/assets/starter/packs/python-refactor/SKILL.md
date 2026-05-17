---
name: python-refactor
description: Safe refactors for Python services with tests-first workflow.
---

# Python Refactor

Use this skill when refactoring Python code that has at least partial test coverage. The workflow is:

1. Identify the boundary you are refactoring (a function, a class, or a module's public API).
2. Run the existing tests for that boundary once to confirm a known-good baseline: `pytest <path> -x --no-header`.
3. Make the smallest safe change that preserves current behavior.
4. Re-run the tests; if they pass, commit.
5. Only after the refactor is green do you add new behavior on top.

See `CONTRACTS.md` for how to identify the public-API boundary that must be preserved, and `TESTS.md` for the minimum test-run scope before and after each refactor step.
