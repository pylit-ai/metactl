# Private skill discovery trials

This optional evaluation lane records bounded metadata for Codex, Omnigent and
Pi skill discovery. It does not change native skill menus, provider consent or
the deterministic route. Keep the ledger and every report outside the repository
on an operator-controlled private volume. No data is sent by the report command.

## Run and inspect

Give a host process a private ledger path using the host's trial option. The
same path can collect baseline, shadow and advisory sessions. Use a unique run
identifier for an evaluation run and compare equivalent tasks only after
checking how they were assigned. Each `metrics.session_id` identifier returned
by the host and stored in the ledger is the SHA-256 digest of the private
session key; it is not the original key.

```sh
metactl skills trials report \
  --log /private/path/skill-discovery-trial.jsonl \
  --output /private/path/skill-discovery-trial.html \
  --json-output /private/path/skill-discovery-trial-summary.json
```

The HTML file is self-contained and can be opened locally. `--runtime codex`,
`--runtime omnigent` or `--runtime pi`, and `--arm baseline`, `--arm shadow` or
`--arm advisory` filter the report. Files are created mode `0600` and must
remain owned regular files with one link. Symlink or hardlink targets, unsafe
permissions, malformed records, incomplete lines and conflicting duplicate
event identifiers fail closed. Recording errors must be surfaced by the caller;
an absent event is unknown data, not a successful measurement.

Attach a verified task result only after checking an independent verifier. The
`verifier_ref` is a SHA-256 digest of a private verifier record, never a path,
transcript or URL. A pass or fail requires that reference; `unknown` does not.
Use `metrics.session_id` from the host response. Do not hash it a second time.
The CLI infers the run and transport from exactly one recorded match. If a
session appears in more than one run, pass `--run-id` and, if needed,
`--transport` from the host metrics.

```sh
metactl skills trials outcome \
  --log /private/path/skill-discovery-trial.jsonl \
  --session-id SESSION_SHA256 --runtime codex --arm advisory \
  --success pass --verifier-ref VERIFIER_SHA256 \
  --task-ms 123456 --input-tokens 12000 --output-tokens 1500
```

Optional outcome fields are `--cost-usd` and `--human-interventions`. Supply
cost only from a billing or independently measured source. Missing values stay
unknown; they are never treated as zero. The outcome command records the
operator-supplied verification reference but does not itself re-run that
verifier.

## What the report measures

The report groups runtime and arm. It shows sessions, discovery/load/outcome
counts, fallback/reorder/abstain counts, provider attempts and observed calls,
known usage coverage, p50/p95 measured latency, returned bytes, repeated loads,
task completion labels, task duration and cost coverage. Unknown provider calls
and missing outcome/usage/cost data stay explicit. Provider attempts may have
incurred charges when validated usage is unavailable.

These are descriptive cohorts. Without randomized or matched task assignment,
their difference is **not** a causal saving. Result bytes are not prompt
tokens. A skill host's small tool interface does not prove that the native
catalog was withheld from model context. The discovery event records
`native_catalog_suppressed: false` and `cost_usd: null` until separate evidence
establishes those facts; outcome cost is independently supplied.

The ledger allowlist contains numeric metrics, bounded model and reason codes,
and opaque hashes or UUIDs. It rejects raw queries, skill names or bodies,
credentials, error text, paths and arbitrary extra fields. Keep the ledger
private anyway: hashes and run metadata can still reveal activity patterns.
Retention and deletion remain an operator decision. Do not commit or publish
actual logs or reports.
