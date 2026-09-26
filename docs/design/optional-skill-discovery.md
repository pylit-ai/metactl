# Optional skill discovery: implementation and preregistered evaluation

Status: implementation contract, frozen before benchmark execution.

## Scope and ownership

Keep ordinary compilation and installed skills unchanged. Add a deterministic,
read-only project-bound catalog/search/load API. A small optional host adapter
exposes two MCP tools and may ask Jev to reorder ambiguous candidates. No API
key, network, SDK, account, or model is required for deterministic use. Model
calls never enter the Rust compiler or change eligibility. Native integration
is opt-in; an MCP server alone cannot hide a host's existing skill catalog.

Only plain instruction packages supported by the adapter are eligible. Denied,
quarantined, confirmation-required, manual-only, disabled, target-incompatible,
blocked, or native-effect-dependent packages fail closed. Exact original content,
canonical containment and package digests are verified again on every load.
No model argument may supply a project root, permissions, or invocation origin.

## Dependency plan

1. Freeze evaluation contract and safe synthetic fixtures (this document).
2. Implement project-bound deterministic catalog/discovery/loading and tests.
3. Implement optional host adapter, tiny tool surface, Jev fallback tests.
4. Benchmark the real CLI and adapter; independently review security and claims.
5. Run repository gates and publish a reviewed task branch/draft PR, not deploy.

Independent review is required before promotion. A failure patches the affected
node; do not weaken gates or replace missing host evidence with disk counts.

## Metrics and comparisons

Compare A: existing native-style catalog/route; B: deterministic discovery;
C: the same discovery with optional Jev. Report C as not measured unless live
calls are separately authorized. Fake provider responses prove integration only.

Capture per case, not only aggregates:

- candidate recall@5: fraction of labeled relevant skills in five results;
- precision@5 and mean reciprocal rank; multi-skill and no-match cases separate;
- exact-name retrieval, unauthorized loads, source fidelity, stale-digest refusal;
- discovery/load latency p50/p95, cold subprocess startup included;
- full catalog bytes versus bootstrap/tool-schema bytes and returned metadata;
- repeated tool loads and tool-result bytes, not assumed token counts;
- enabled/disabled/missing-key/deadline/API-error/budget/invalid-answer fallback
  identity and bounded latency; failure must not suppress a baseline error;
- actual provider usage/model/calls/fallback reason when available; unknown cost
  remains null, never zero. Avoid logging queries, bodies, credentials or paths.

The offline set consists of safe authored tasks and expected skill names fixed
before ranking runs, including descriptions unlike skill names. A synthetic
large roster measures serialization scaling, not realistic task quality.
Report all failures and sample size. This development suite is not held-out
evidence and cannot justify a default change. Do not tune on it then call it a
held-out benchmark.

## Gates

- Zero forbidden loads or changed instruction bytes in deterministic fixtures.
- Exact lookup works; manual/disabled/revoked/missing/stale/escaping inputs fail.
- Every unavailable Jev path retains the complete deterministic result.
- Tool list contains exactly two tools and no catalog entries or per-skill enum.
- Median and tail latency and recall are reported even when results regress.
- No production/default promotion based on this offline suite.

Before a native rollout: prove bulk descriptors absent in an actual serialized
host request, preserve required governance and native semantics, then run
independently labeled held-out complete sessions. Measure verified task success,
required-skill recall, unnecessary reads, retries, compactions, total input,
cached/write/output tokens, actual price and p95 task latency. Predeclare a
non-inferiority margin and sufficient sample size after baseline collection.
Jev earns adoption only if it improves this outcome beyond deterministic mode.
Caching means fewer catalog bytes do not imply proportional dollars or latency.

## Privacy and rollback

Network ranking is disabled by default, explicit data-sharing opt-in and a
finite call budget are required, and no interactive secret resolution occurs.
Keep provider-specific evaluation private unless the applicable agreement allows
publication. Restore native discovery by removing the optional adapter; no
global files, native catalogs, model routing, or compilation defaults are changed.
