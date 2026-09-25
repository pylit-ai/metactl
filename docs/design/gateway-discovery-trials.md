# Gateway discovery trials

Implement optional project-gateway ranking, a common private event ledger, and
explicit adapters for Codex, Omnigent and Pi. Keep the existing deterministic
catalog eligibility and original instruction loading authoritative.

## Acceptance and experiment fixed before trials

Compare baseline (no provider), shadow (provider advice recorded, baseline used),
and advisory (validated ordering used). Start with shadow. A provider may change
first position only; every eligible candidate remains recoverable. No automatic
credential provisioning, account migration or native skill-root removal.

The gateway client owns project credentials and shared spend limits. The host
owns a smaller per-process attempt ceiling, explicit data consent/class, and a
wall-clock deadline. Public code must contain no deployment names, secrets or
private evaluation results. Permitted-data declarations apply to the complete
task query and all candidate descriptions, not just the query.

Record per discovery: arm, runtime, opaque session/run/event identity,
baseline/effective/proposed IDs, catalog digest, result bytes, latency, outcome,
attempts, validated provider calls/model/usage and unknown billing. Record loads
and independently supplied session outcomes separately. Never infer prompt
tokens from bytes, task success from provider validity, or savings from cohort
means. Missing sessions/outcomes and uncertain dispatches remain visible.

Promotion requires zero forbidden loads or changed instruction bytes, complete
fallback on unavailable/invalid/budget paths, no unexpected egress, preserved
candidate recall and outcome coverage. A useful later comparison randomizes
representative tasks between baseline and advisory, fixes task/verifier/model,
and measures verified success, total input/cache/output tokens, actual cost,
human interventions and task latency. No improvement verdict from a handful of
synthetic smoke calls. Regressed quality or unresolved missing data rejects a
default rollout; shadow remains optional.

## Work graph

1. Inspect runtime adapters and existing contracts (read-only worker).
2. Add gateway transport and baseline/shadow/advisory host (lead).
3. Add protected ledger, outcome join and standalone results view (isolated worker).
4. Integrate supported host adapters and targeted failure/end-user tests.
5. Independent review, bounded synthetic live acceptance, private report.
6. Push reviewed implementation and integration receipts; no default promotion.

This is bounded implementation and acceptance, not an optimization hypothesis
search; ordinary dependency planning is sufficient. At most two workers plus
lead until review; no paid optimization loop. Live synthetic acceptance is
bounded to four gateway attempts (at most $0.02 reserved exposure under the
existing policy), with no retries or limit increases. Production skill/task
text is never reclassified as synthetic for a smoke check.
