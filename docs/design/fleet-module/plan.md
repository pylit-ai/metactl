# Plan

1. Capture exact base and existing fleet behavior.
2. Move cohesive fleet implementation with function bodies unchanged; expose
   only the internal functions and data needed by existing callers.
3. Add adverse user-path characterization and baseline/candidate differential
   tests comparing output, exit code, file contents and locks.
4. Enforce architecture checks in CI with an explicit module budget.
5. Run full tests, public-boundary/contracts/install smoke and independent review.
6. Publish a separate refactoring PR before changing fleet semantics.

Replan: once the main-file violation was repaired, the same gate revealed the
registry also exceeded its existing limits. Extract cohesive registry discovery
and instruction-formatting helpers as a parallel mechanical change; give the
new modules explicit budgets and preserve all old limits. Run Cargo verification
serially within each target directory so rebuilding a CLI cannot race tests that
are spawning it.

Replan on any unintentional difference; never update expectations to hide it.
