# Optional skill discovery

Experimental, opt-in, plain-instruction adapter. Deterministic mode is the default
and requires no provider, account, API credits, SDK or network access. Existing
compilation, native skill menus and installed skill folders are unchanged.
The optional packaged host needs Python 3.10 or newer on the client machine.

<a id="quick-start"></a>
## First-run workflow

There are three separate steps: make a project catalog available, register the
two-tool host in a coding client, and have an agent call it when specialist
instructions are useful. Jev is an optional fourth step for authorized ranking.

1. Point MetaCTL at an **absolute project path** and run
   `metactl --project /absolute/path/to/project skills host --status --ranker deterministic --trial-mode baseline`.
   Look for `project_ready: true` and an eligible-skill count. `provider_verified:
   false` is expected in baseline mode; status makes no provider request. If
   your machine default profile is unrelated to this project, repeat with
   `metactl --project /absolute/path/to/project --no-profile skills host --status --ranker deterministic --trial-mode baseline`.
2. Preview and apply a target-native registration with the same project and
   profile choice. The preview names the exact file, mode, log and rollback;
   it changes nothing. Apply adds the `metactl-skills` server entry and prepares
   a private state directory for its event log:

   ```sh
   metactl --project /absolute/path/to/project skills connect --target codex-cli
   metactl --project /absolute/path/to/project skills connect --target codex-cli --apply
   metactl --project /absolute/path/to/project skills doctor --target codex-cli
   ```

   Use `--scope user` for a user-wide Codex registration that always points to
   this one project. Project scope is the default; Codex loads project config
   only when the project is trusted. The connector supports project config for
   `codex-cli`, `claude-code`, `cursor`, `gemini-cli`, and `opencode`.
   In a Git project, an unignored config containing machine-specific paths
   requires explicit `--allow-unignored` on apply. Prefer a reviewed local Git
   exclusion for that config. Tracked configs are refused, including a tracked
   user-scope Codex dotfile.
   `openclaw`, `filesystem-agent`, Pi and Omnigent have manual adapters; the
   connector reports that limitation explicitly. The entry is deterministic
   baseline with `provider_calls=0`; it does not turn on Jev. A private event
   log under the user's state directory is prepared on apply. If status needed
   `--no-profile`, pass it before `skills connect` so the registration retains it.
   `connect` resolves `python3` to an absolute path from the current `PATH` so
   GUI clients do not silently choose a different interpreter. If Python is
   outside `PATH`, add `--python /absolute/path/to/python3` to `connect`; the
   path must remain available to the agent. The printed rollback
   command recognizes the managed baseline entry even when the executable or
   Python path later changes. `doctor` compares the registration with the
   current invocation. It checks host readiness only when they match, using
   the current shell environment; otherwise readiness is unknown. The local
   catalog count is checked separately, so it can remain known when host
   readiness is unknown. If Python is missing or the client config is a
   symlink, `doctor` still reports the other states. A failed host check is
   labeled `host_failed` because the cause can be Python, profile or project
   setup. Doctor flags missing registered executable and Python paths without
   running the client-configured command; rerun `connect --apply` after an
   upgrade to repair those paths.
   A user-wide Codex registration shares this one project's skill catalog and
   event metadata across Codex sessions in other repositories; choose project
   scope when catalogs or visibility should remain separate. The baseline
   ledger does not store query text or skill bodies.
   Existing JSON client files are parsed and re-serialized, so formatting
   and key order may change. Integers outside the exact signed/unsigned 64-bit
   range are refused to prevent a lossy rewrite; edit such a file manually. JSONC
   comments are not edited automatically. OpenCode's `opencode.jsonc` is
   detected to avoid creating a second config file. Existing file permissions
   are preserved. Removing the managed entry
   is immediate with `--remove` (no `--apply` needed). It leaves the private
   event log and may leave an empty client config object. The printed rollback
   uses the installed binary's absolute path; if that binary is later removed,
   run the same command with a currently installed `metactl` binary.
   Package managers may replace that binary during an upgrade. Rerun
   `skills connect --target <target> --apply` afterward, then `skills doctor`,
   to refresh the client registration; a versioned executable path can stop
   launching after its old version is removed.
   Reapplying with a different profile, config, overlay or log destination
   requires an explicit preview with `--replace`, followed by `--apply --replace`.
   Changing only the installed binary or Python path updates the managed entry.
   For `--exclude-skill` or a custom client, use `skills host --client-config`
   and the manual adapter guide.

   **Approved public data only:** `connect` can also register gateway-backed
   `shadow` or `advisory` mode when the project and query are authorized for
   `public-nonsensitive` data. Use the project's scoped gateway client, an
   explicit data-transfer opt-in, and bounded attempts:

   ```sh
   metactl --project /absolute/path/to/public-project skills connect \
     --target codex-cli --trial-mode advisory --allow-provider-data \
     --gateway-project approved-project-id \
     --gateway-data-class public-nonsensitive \
     --gateway-command /absolute/path/to/jev \
     --max-provider-calls 2 --provider-deadline 1
   # Review the preview, then repeat with --apply.
   ```

   **Data sent to the gateway:** each discovery request can send the task query,
   candidate skill names and descriptions, the gateway project ID, and the data
   class. The query may contain text the agent copied from your project. Do not
   enable this option for private projects, secrets, or restricted customer and
   third-party work under the current public-data authorization. This opt-in path does
   not authorize private-project traffic or a fleet-wide, default-on rollout.
   MetaCTL cannot verify that a project or query is public; the data class is
   your attestation.

   `--max-provider-calls` permits 1-10 attempts per host process, not per
   discovery request; `--provider-deadline` must be positive and no more than
   5 seconds per attempt. The example uses 2 attempts and 1 second. `shadow`
   records Jev's proposed order without changing the returned order;
   `advisory` may apply a validated proposal. Connect and doctor call only the
   local host's offline status path, never the provider. A later discovery call
   may contact the gateway. On unavailable, rejected, timed-out or over-budget
   responses, the host returns deterministic ordering and records whether the
   provider attempt or billing state is unknown. Policy changes require
   `--replace`. Synthetic classification is reserved for `skills host --check`.
   To check actual use, run `skills doctor` with the same routing flags and
   inspect `registered_mode`, `registered_gateway_command_state`, and `routing`.
   The gateway command state detects a missing or non-executable client without
   calling it. For a specific discovery request,
   inspect its `routing_receipt` for `reason` and `provider_calls`, then match
   its session/run ID in the private event log. A configured registration or
   healthy host alone does not prove that the agent called Jev. Use `--json
   --full` when copying the complete generated argument array; ordinary JSON
   output marks long arrays as truncated.
3. Restart or open a fresh agent session. Confirm both `discover_skills` and
   `load_skill` are available, then request one harmless ambiguous discovery
   and load a returned ID with its digest. The `routing_receipt` should say
   `mode=baseline`, `provider_calls=0`, and `log=recorded` when the private
   ledger is writable. This checks the live tool path without using Jev.

If the tools are not visible, check that the project is trusted by the client,
the registered project and executable paths are absolute and exist, Python
3.10+ is available, and you opened a new agent session after registering.
If discovery works but `log=failed`, check that the private log's parent
directory exists and is writable. `skills doctor` separates local host
readiness, registration, observed discovery events, and unknown client or
benefit states; it makes no provider call. A missing event does not prove the
agent skipped discovery. Doctor reads the most recent 2 MiB of the private log,
reports malformed lines as partial evidence, and marks a truncated window;
older events may need `skills trials inspect`. `--status` confirms only local
host readiness.

For daily work, discover when the task or phase calls for unfamiliar specialist
instructions; use an exact known skill directly when appropriate. A project may
add this **optional** line to its `AGENTS.md` after registering and verifying
the host:

> When specialist instructions may help, call MetaCTL `discover_skills` if
> available, load a relevant result by ID and digest, and show its
> `routing_receipt`. If discovery was not called, say so when reporting routing.

This line influences agent tool choice. It does not install a server, change
Jev consent or client permissions and approval settings, or prove that an agent
called the tool. The starter
`skill-discovery` pack carries the reusable agent instructions for projects
that select it. Its current starter-pack projection is limited to `codex-cli`;
other supported MCP clients can register the host separately. An `AGENTS.md` line
is a short project-level reminder, not a requirement for every MetaCTL user.

Read evidence in this order: client registration, fresh-session tool list,
actual `routing_receipt`, private event log, then measured task outcome. A
`provider_calls=1` receipt proves one validated Jev response was observed, not
that it improved the task. See [private trials](discovery-trials.md) for log
inspection. Missing events do not prove discovery was unused.

## Direct CLI discovery

Use an installed MetaCTL CLI, or build from source with
`cargo build -p metactl`. In a configured project:

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
HTTP listener. `skills connect --scope user --apply` modifies Codex user
configuration; the default project scope modifies only the selected project's
client file.

Host-disabled IDs or names must be mapped by the operator's adapter with repeatable
`--exclude-skill NAME_OR_ID`. These restrictions apply to discovery and direct load.
The server cannot inspect arbitrary host settings: do not enable it for a library
whose native restrictions have not been mapped. Plain text retrieval is not native
activation and never grants tool permissions. Exact skill names can be searched;
ambiguous same-name results retain their distinct IDs and source digests.

## Shared gateway and measured trials

Use the approved scoped gateway client when your environment provides one. The
client owns project authentication and shared quotas; MetaCTL never needs its
credential or the upstream provider key. Install the gateway client through your
organization's onboarding process first. MetaCTL does not provision it.

```sh
metactl --project /path/to/approved-project skills host \
  --ranker jev --jev-transport gateway --gateway-command /path/to/jev \
  --gateway-project APPROVED_PROJECT_ID --gateway-data-class public-nonsensitive \
  --allow-provider-data --max-provider-calls 4 --provider-deadline 5 \
  --trial-mode shadow --runtime codex-cli \
  --event-log /private/path/discovery.jsonl --status
```

Replace `--status` with `--check` for one fixed synthetic request, or omit it to
run the persistent tool server. A check consumes the same gateway budget as a
discovery attempt. Use `--client-config` to produce registration arguments and
the [agent adapter guide](discovery-agent-adapters.md) for Codex, Omnigent and Pi.
The data classification applies to **both the query and candidate descriptions**;
do not label private project content as synthetic or public. If those inputs are
not approved, use the deterministic baseline without provider consent.

Choose `--trial-mode baseline` for no provider dispatch, `shadow` to record Jev's
proposal while returning the baseline, or `advisory` to use validated ordering.
Missing client/configuration, rejection, deadline and exhausted budgets retain
the original candidates. No retry or interactive login occurs. The host attempt
ceiling resets per process; the gateway must enforce cross-process monetary
limits. Local readiness is not proof that gateway credentials are accepted.

`--event-log` is opt-in private metadata recording. Check
`metrics.telemetry_status`; a failed write does not interrupt discovery but must
not be counted as a measured result. See [private trials](discovery-trials.md)
for the local HTML dashboard and independently supplied task outcomes. Compare
equivalent tasks across baseline, shadow and advisory sessions before claiming
benefit; native catalog suppression and prompt-token savings remain unproven.

## Direct-provider install and verification

Install the released CLI using the [README installation choices](../../README.md#install).
The host is embedded in both crate and binary distributions. Install Python 3.10+
on PATH; no SDK or repository checkout is required. Existing direct-script usage
below remains supported for developers.

If system `python3` is older (common on macOS), select an existing compatible
interpreter with `skills host --python /path/to/python3.12` or
`METACTL_DISCOVERY_PYTHON`. Status reports its version/path and generated client
configuration pins that interpreter. MetaCTL does not install Python automatically.

1. In a configured project, run `metactl skills host --status`. This reads local
   readiness only. First use may materialize the bundled library cache; it does
   not send provider data or change native skill roots.
2. Make `TYPESAFE_API_KEY` available through your approved runtime secret injector.
   Do not paste keys into commands, client configuration, transcripts or repos.
   A secret-manager reference resolving successfully is not API verification.
3. Explicitly choose data consent and a per-process request ceiling:

   ```sh
   metactl --project /path/to/project skills host --ranker jev \
     --allow-provider-data --max-provider-calls 20 --status
   metactl --project /path/to/project skills host --ranker jev \
     --allow-provider-data --max-provider-calls 1 --check
   ```

   `--check` sends only a fixed synthetic example, not project content. Exit 0
   requires a validated provider answer; missing key, disabled integration,
   deadline or invalid response exits 1. A valid `none` answer proves availability,
   not task quality. Status alone always reports `provider_verified: false`.
4. Run the same enabled command with `--client-config` instead of `--status`.
   Review the printed `mcpServers.metactl-skills` command/arguments and add them
   through the client's supported MCP settings. The snippet contains no key.
   Inject the key into the child process using the client's supported secret
   environment mechanism. Restart that connection; an existing running child
   will not pick up environment or binary changes automatically.
5. Ask the client to call `discover_skills` for an ambiguous task with at least
   two eligible results. Confirm its returned `metrics.provider_calls: 1`, the
   pinned `metrics.model`, and non-null validated usage. `metrics.ranker: jev`
   means accepted advisory ordering. `reason: abstained` means a valid provider
   answer retained the baseline. Disabled, missing-credential, budget, deadline
   and schema failures are explicit fallback—not evidence Jev was active.

| Signal | What it proves |
| --- | --- |
| `configured_ranker: jev` | Operator selected the optional route |
| `provider_ready: true` | Local prerequisites present, not API availability |
| `--check` exit 0, `provider_verified: true` | One synthetic call passed now |
| Real tool metrics with model, usage, one call | That actual discovery request reached a validated provider result |

For managed environments, `METACTL_SKILL_RANKER=jev`,
`METACTL_JEV_ALLOW_DATA=true`, and `METACTL_JEV_MAX_CALLS=20` configure the packaged
CLI. Explicit flags override values. These variables are not secrets. A budget
resets with each child process; this is not a shared daily or dollar limit.
Use the gateway transport above when organizational policy requires a properly
admitted project gateway; direct transport does not implement project auth.
Never distribute a shared provider key merely to make remote checks pass.

Rollback: remove the MCP registration or set `--ranker deterministic`, then
restart the connection. Deterministic catalog/discover/load commands never use
Jev, even when the host environment enables it. Enabling Jev does not make every
MetaCTL command or conversational turn call a model.

## Direct-script optional Jev

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
python3 -m unittest discover -s tests -p 'test_skill_discovery*.py' -v
python3 scripts/benchmark_skill_discovery.py --repeats 3 --distractors 200
```

Metrics travel in tool responses, with optional private local logging. They include
local latency, returned bytes/count, repeated-load flag, catalog digest, ranker,
fallback reason, provider attempts, validated provider calls, model and reported usage. `provider_calls`
is `null` for an uncertain attempt (previously counted as a call); inspect
`provider_attempts` as well. Shadow responses hide the proposal and use the
`shadow` reason; the private ledger preserves the underlying ranking reason. They exclude
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
