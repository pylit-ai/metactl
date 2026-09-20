# Optional skill discovery

Experimental, opt-in, plain-instruction adapter. Deterministic mode is the default
and requires no provider, account, API credits, SDK or network access. Existing
compilation, native skill menus and installed skill folders are unchanged.

## Quick start

Build `cargo build -p metactl`. In a configured project:

```sh
metactl --project /path/to/project --json --full skills catalog
metactl --project /path/to/project --json skills discover "investigate reconnect failures"
metactl --project /path/to/project --json skills load DISCOVERED_ID --digest DISCOVERED_DIGEST
```

For private task descriptions use `skills discover --query-stdin` and send the
description on stdin, not command-line arguments. `catalog` is an administrative
inventory command; do not inject its entire output into the agent prompt.

Run the optional host adapter as an MCP stdio child (Python 3.10+ standard library):

```sh
python3 /path/to/metactl/scripts/skill_discovery_host.py \
  --metactl /path/to/metactl/target/debug/metactl \
  --project /path/to/project
```

Register that command and argument array using your client's supported MCP
configuration. The project is fixed by the operator; model tool arguments cannot
change it. The server exposes only `discover_skills(query)` and
`load_skill(id, digest)`. It is session-bound, not an always-on service, and has no
HTTP listener. No installation command modifies user/global client configuration.

Host-disabled IDs or names must be mapped by the operator's adapter with repeatable
`--exclude-skill NAME_OR_ID`. These restrictions apply to discovery and direct load.
The server cannot inspect arbitrary host settings: do not enable it for a library
whose native restrictions have not been mapped. Plain text retrieval is not native
activation and never grants tool permissions. Exact skill names can be searched;
ambiguous same-name results retain their distinct IDs and source digests.

## Optional Jev

Only after approving outbound task/description data and a provider request budget:

```sh
python3 /path/to/metactl/scripts/skill_discovery_host.py \
  --metactl /path/to/metactl/target/debug/metactl --project /path/to/project \
  --ranker jev --allow-provider-data --max-provider-calls 20 --provider-deadline 1.5
```

Provide `TYPESAFE_API_KEY` through your existing secret-injection mechanism. Never
put it in argv, a skill, checked-in configuration or model input. Merely selecting
`--ranker jev` does not authorize data movement: without the data flag, key and
positive call budget it uses the deterministic baseline. Budget is per process;
it includes failed attempts but is not a cross-process monetary quota.

The host uses the documented [TypeSafe API](https://docs.typesafe.ai/api), pinned
to `jev-1.13.0`. One call may move a candidate to the first position; it cannot add,
drop, activate or authorize a skill. None, failure, invalid schema, missing key,
exhausted budget or deadline returns the original ordering. No retries or login
prompts. Clear exact matches and fewer than two candidates do not call Jev. The
default deadline is 1.5 seconds for the provider subprocess, in addition to local
discovery time. A configured model/API change needs contract verification.

## What this does and does not save

For a **new isolated Codex project**, select the starter `skill-discovery` pack
plus your required governance/core packs in `metactl.yaml`, leaving specialists
in the configured off-root library but out of `packs`. Then preview with
`metactl sync --preview` and use normal reviewed sync. Existing compilation
already projects only selected packs; no new probabilistic compiler mode is
needed. A regression test proves a core-only project emits one bootstrap SKILL.md,
omits a specialist sentinel from root instructions, and still discovers that
specialist through the CLI. This proves generated project files, not a captured
native model request. Multi-skill packs remain indivisible for this recipe; do
not drop a governance pack to hide its specialists. User/plugin/legacy roots are
unaffected and can still contribute catalog entries.

The MCP tool list is constant-size and contains no catalog names, descriptions or
per-skill enum. This is an implemented external discovery interface—not proof that
your client stopped injecting its native catalog. When both remain installed,
you may pay MORE context/tool-call overhead. Do not default this feature on yet.

For context savings, a separate host integration must expose the small discovery
interface while withholding eligible specialist descriptors *before* constructing
the request. Keep mandatory governance and unsupported native skills visible.
Inventory project, ancestor, user, legacy and plugin scopes; preserve manual
commands and explicit-only/disabled settings. Do not rename/delete installed skill
trees, set manual-only flags to bypass their semantics, or assume a refreshed
picker proves prompt exclusion. Current native suppression/activation remains an
unpassed rollout gate; this experimental server alone is not that adapter.

## Supported scope and safety

Source libraries and project/profile configuration must be locally trusted. This
is not a sandbox against a malicious process with write access to those roots.
Declared paths are canonicalized and must remain within the library and package.
Each resource is bounded to 1 MiB. Missing resources fail closed. Descriptions use
YAML parsing, including multiline scalars. Original instruction text is never
summarized, fabricated or truncated; the same captured bytes are hashed and sent.

Discovery rejects unpromoted/rejected/retired provenance, confirmation-required
and pack-approval-required entries, incompatible role/target, policy suppression,
disabled/manual-only flags, native sidecars, hooks, native-effect frontmatter and
declared prerequisites. A blocked surface conservatively excludes its whole pack.
Packages requiring these behaviors must remain on their native path. Each public
catalog/load call refreshes source manifests and policy; no stale decision cache
is used. A digest covers declared package resources. File references are returned
for the host to read under its existing permissions; arbitrary undeclared external
references are not thereby admitted or verified.

## Metrics and reproducible offline checks

```sh
cargo build -p metactl
python3 -m unittest discover -s tests -p test_skill_discovery_host.py -v
python3 scripts/benchmark_skill_discovery.py --repeats 3 --distractors 200
```

Metrics travel in tool responses, with no automatic external logging. They include
local latency, returned bytes/count, repeated-load flag, catalog digest, ranker,
fallback reason, provider calls, validated model and reported usage. They exclude
raw queries, credentials and bodies. Failure may have incurred a provider charge
even when usage is unknown. No cost is invented from unknown usage.

The benchmark compares existing routing with the real deterministic CLI/host on
12 authored development cases plus synthetic distractors. It reports every run,
recall/precision at five, reciprocal rank, no-match accuracy and p50/p95 latency.
Repeated runs are timing samples, not additional independent quality examples.
Metadata bytes are not prompt tokens; these measurements are not coding task
success, verified latency gains, dollar savings or live Jev quality.

The [evaluation contract](../design/optional-skill-discovery.md) defines the
remaining held-out session gates before rollout. Keep live Jev benchmark data
private unless its applicable agreement permits publication. Rollback simply
removes the optional MCP registration; no native defaults have changed.
