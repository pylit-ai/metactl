# Private skill discovery trials

This optional evaluation lane records bounded metadata for all standard MetaCTL
target labels and the additional Omnigent and Pi adapters. See the
[compatibility boundaries](discovery-agent-adapters.md). It does not change native skill menus, provider consent or
the deterministic route. Keep the ledger and every report outside the repository
on an operator-controlled private volume. No data is sent by the report command.

## Run and inspect

**Registration is not automatic routing.** If the coding agent never calls
`discover_skills`, there is no discovery event and no Jev call through this
host. `--ranker deterministic` or `--trial-mode baseline` makes zero provider
calls. Shadow calls Jev when eligible but preserves baseline order; advisory
can apply a validated order. This feature selects skills, not the coding
agent's underlying model.

Every discovery response includes a `routing_receipt`, for example:
`Jev discovery: mode=shadow; reason=shadow; provider_calls=1; order_changed=False; log=recorded; event=<opaque ID>`.
In shadow mode the response omits proposed IDs and masks successful choice or
abstention reasons as `shadow`, so the coding agent cannot follow the hidden
proposal. Failure and no-call reasons remain visible. The private ledger
retains the proposal and original reason for later analysis.
The host bootstrap asks the agent to surface it; a client may ignore that
instruction or hide tool output, so inspect the actual tool response or ledger.
No global hook or guarantee of discovery on every coding run is installed.

| Evidence | Meaning |
| --- | --- |
| `provider_attempts=0`, `provider_calls=0` | Jev not called; inspect `reason` (baseline, disabled, unambiguous, budget, consent, etc.). |
| `provider_calls=1`, model and usage present | Validated Jev response observed; this is not proof of useful task impact. |
| `provider_calls=null` | Attempt occurred but provider completion is unknown; a charge may still have occurred. |
| `reason=unchanged` | Valid Jev choice agreed with the first baseline candidate. |
| `reason=abstained` | Jev returned no preference; baseline retained. |
| Different proposed IDs | Suggested ordering changed; applied only in advisory mode. |
| Different effective IDs | Returned ordering actually changed. |
| `telemetry_status=recorded` | This event was appended to the private ledger. `failed` means inspect permissions/size locally; `disabled` means no path configured. |

Keep `metrics.event_id`, `metrics.run_id` and `metrics.session_id` with the
coding task's local trace to correlate records. `run_id` identifies a host
process, not necessarily a whole coding task. Use a unique opaque
`--session-id` when launching each trial and retain its returned digest. Do not
put task text in identifiers. Native clients can reuse hosts across turns;
their lifecycle is not automatically equivalent to a trial session.

```sh
metactl skills trials inspect --log /private/path/discovery-events.jsonl \
  --session-id SESSION_SHA256 --run-id RUN_UUID_HEX
```

The run filter is optional. `no_recorded_events` means no matching evidence,
not verified non-use: tools may not have run, logging may have failed, or the
wrong ledger/session may have been selected. Missing or invalid ledgers fail
with an error. Inspect prints validated private metadata; redirect only to a
private destination. Report generation and inspection never call Jev.

For visible operation, ask your agent: “For relevant skill discovery, call
discover_skills and show its routing_receipt. If you do not call it, say
discovery was not invoked. Report logging failures.” This is a visibility
instruction, not permission to transmit private task or skill descriptions.

### Storage and later analysis

`--event-log` is explicit: there is no default central collection or upload.
Use one durable private directory for logs, reports and your task-to-session
mapping. Logs include time, runtime, mode, opaque identifiers and bounded
metrics; never raw queries or skill bodies. The ledger is capped at 8 MiB;
when full, recording reports `failed`. Choose a new private ledger at a session
boundary and preserve the old one for analysis. No automatic rotation or
deletion occurs. Coding-agent transcripts are separate client-owned records.

The private trial ledger currently requires POSIX file ownership and `flock`
(macOS/Linux); Windows is not supported. Use physical absolute paths: macOS
`/tmp` and `/var` are symlinks, so use `/private/tmp` or resolve the chosen
directory first. Symlink ancestry is rejected rather than silently followed.
Concurrent lock contention fails immediately and is reported as logging
failure; discovery still returns its normal result. Inspect again after the
writer finishes. A static `--client-config` deliberately omits `--session-id`;
default hosts get a fresh random session identity at launch.

`skills host --call-tool discover_skills` accepts a JSON argument object on
stdin for one-shot diagnostics. Every invocation starts a fresh process.
Provider-enabled one-shot calls require gateway transport with an approved
shared budget; direct provider transport is rejected for this mode. Baseline
one-shot calls remain available without provider access.

Give a host process a private ledger path using the host's trial option. The
same path can collect baseline, shadow and advisory sessions. Use a unique session
identifier at each trial launch and compare equivalent tasks only after
checking how they were assigned. Each `metrics.session_id` identifier returned
by the host and stored in the ledger is the SHA-256 digest of the private
session key; it is not the original key.

```sh
metactl skills trials report \
  --log /private/path/skill-discovery-trial.jsonl \
  --output /private/path/skill-discovery-trial.html \
  --json-output /private/path/skill-discovery-trial-summary.json
```

The HTML file is self-contained and can be opened locally. `--runtime codex-cli`,
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
  --session-id SESSION_SHA256 --runtime codex-cli --arm advisory \
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

The reorder count requires a changed proposed ID order. In shadow mode this is
only a proposal; in advisory mode it is the applied order. A valid Jev choice
that was already first is recorded as `unchanged`, not as a reorder or fallback.
The report also checks ID order when reading older events whose reason was
incorrectly recorded as `reordered` for an unchanged choice.

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
