# Public API and boundaries

Before refactoring, identify public entrypoints (modules, RPCs, CLI flags) and treat them as contracts. Prefer additive changes; if you must break callers, document the migration path and update dependents in the same change when feasible.
