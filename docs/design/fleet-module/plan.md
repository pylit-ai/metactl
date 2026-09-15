# Plan

1. Capture exact base and existing fleet behavior.
2. Move cohesive fleet implementation with function bodies unchanged; expose
   only the internal functions and data needed by existing callers.
3. Add adverse user-path characterization and baseline/candidate differential
   tests comparing output, exit code, file contents and locks.
4. Enforce architecture checks in CI with an explicit module budget.
5. Run full tests, public-boundary/contracts/install smoke and independent review.
6. Publish a separate refactoring PR before changing fleet semantics.

Replan on any unintentional difference; never update expectations to hide it.
