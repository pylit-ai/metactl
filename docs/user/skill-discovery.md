# Optional skill discovery

Experimental, opt-in, plain-instruction adapter. Deterministic mode is the default
and requires no provider, account, API credits, SDK or network access. Existing
compilation, native skill menus and installed skill folders are unchanged.
The optional packaged host needs Python 3.10 or newer on the client machine.

<a id="quick-start"></a>
## Enable Jev once for managed projects

Persistent preferences send a task description and candidate skill metadata
through your scoped gateway to TypeSafe. Provider processing and retention
apply. Exclude secrets and projects whose customer or organization policy
prohibits that transfer. Private repositories you control can use
`private-owned` when the gateway grant permits it; do not relabel them public.

```sh
metactl --project /absolute/path/to/project skills preferences --mode enabled \
  --allow-provider-data --gateway-command /absolute/path/to/jev \
  --enroll --gateway-project approved-project-id --data-class private-owned
metactl --project /absolute/path/to/project skills connect --target codex-cli --use-preferences
metactl --project /absolute/path/to/project skills connect --target codex-cli --use-preferences --apply
metactl --project /absolute/path/to/project skills doctor --target codex-cli --use-preferences --json
```

The enablement choice is saved in `$XDG_CONFIG_HOME/metactl/discovery.json`
(default `~/.config/metactl/discovery.json`), owned by the user with mode 0600.
Enroll another project with `skills preferences --enroll --gateway-project ID
--data-class private-owned`; it inherits the saved default without another
permission prompt. Enrollment matches the canonical project path exactly:
unrelated repositories, nested projects and new worktrees are not silently
enrolled. Existing explicit project opt-outs survive re-enrollment.
Enrollment checks the path/ID binding through the gateway client's free
`check-project` command before saving. Changing an existing data classification
or gateway ID requires `--replace-enrollment`; the gateway still rechecks scope
when evaluating each request.
Use the same `--use-preferences` registration for each supported target.
An existing managed registration requires preview/apply with `--replace`.

`skills preferences --project-mode disabled` disables the current project;
`--project-mode inherit` restores its inherited setting. `--mode disabled`
disables all enrolled projects. `METACTL_JEV_DISABLE=1` disables a launched
host for one session. Preferences are read before each discovery request;
changes affect existing preference-aware hosts without restart. Initial MCP
registration still requires the native agent to reload its tools. A request
already in flight may finish. Legacy registrations using explicit trial flags
must be reconnected with `--use-preferences` to inherit these controls.
Use `skills preferences --revoke-provider-data` to withdraw permission itself;
enabling again then requires the explicit data-transfer choice.

Call/deadline settings are advanced options (`--max-provider-calls 1..10`,
`--provider-deadline` at most 5 seconds). Gateway scope, data restrictions,
shared budgets and expiry remain authoritative; local preferences cannot
override them. A provider error or denial preserves local ordering.
The default four-call ceiling lasts for the entire host process; after four
attempts, further discoveries use local ordering. Restarting the agent starts
a fresh host allowance, but does not reset the gateway's shared limits.
The one-shot `--call-tool` adapter starts a new host each time; its calls are
bounded by the shared gateway limits rather than a persistent local counter.

Doctor reports effective preferences separately from registration and observed
use, without calling the provider. Each discovery returns a `routing_receipt`
with provider calls, fallback reason, order change and log status. Inspect the
private `event_log` path printed by connect/doctor; logs contain metadata, not
task text or skill bodies. Local logging is separate from provider retention.
Enabled does not prove a coding agent invoked discovery: ask it to show its
receipt. Default-on uses Jev when useful; unambiguous requests remain local.

## How skills are selected

Single-word names or aliases inside prose remain topical matches unless marked
with `$name` or backticks. For example, "Review tests for a repair" can still use
Jev; "Use `$review`" explicitly selects that skill. A query consisting only of
the exact skill name or ID also remains an explicit local selection.

Discovery first filters the catalog for project policy and supported instruction
semantics. It then ranks eligible skills locally. Whole skill names and aliases
inside task prose take priority; longer matching labels win over shorter ones.
Direct named exclusions such as `do not use NAME` are respected. Other queries
use distinct meaningful words, ignoring common connecting words such as `and`
and `for`. Repeated aliases/intents do not accumulate extra field weight. A
negative intent penalizes a match only when all its meaningful words are present.
This remains lexical retrieval, not general natural-language understanding.

The host retrieves up to twenty eligible local candidates for Jev while returning
at most five to the agent. A validated choice can promote a candidate from outside
the original five; the remaining returned candidates retain their local order.
`--candidate-limit 5..20` bounds this provider pool; `5` restores the original
shortlist. The packaged host also accepts `METACTL_DISCOVERY_CANDIDATE_LIMIT`.
The existing payload, consent, call and deadline bounds still apply. The host
trims the lowest-ranked tail until the actual encoded payload fits, preserving
full descriptions and the original five. If even five exceed the wire budget,
it makes no provider attempt and returns those original five.
Abstention, failure, deterministic mode and shadow mode also retain those five.
Explicit name/alias matches stay local. `score` is a local ranking value, not a
probability or Jev confidence; scores may change as retrieval improves. `excluded`
counts catalog eligibility/validation rejections, not results omitted by the limit.

Agents should load a relevant result using its returned ID and digest before
following its full instructions. A discovery receipt proves the lookup; a load
event proves delivery of instructions. Neither alone proves task benefit.

## Check status and limits

These checks make no provider request. Use the same absolute project path and
target as the installed connection. The `jev` commands below refer to the scoped
gateway client supplied by your operator; MetaCTL does not provision that service.

| Question | Command or evidence |
| --- | --- |
| What preferences apply to this project? | `metactl --project /absolute/path/to/project skills preferences` |
| Is this client's connection ready, and what discovery events were observed? | `metactl --project /absolute/path/to/project skills doctor --target codex-cli --use-preferences --json` |
| Is the preference-aware local host ready? | `metactl --project /absolute/path/to/project skills host --use-preferences --status` |
| What access, configured budget limits and expiry does the scoped gateway report? | `jev status --project approved-project-id` |
| Does this working directory match the gateway project? | From the intended project directory: `jev check-project --project approved-project-id` |
| What happened in a particular run? | Read its `routing_receipt`, then `metactl skills trials inspect --log /private/path/discovery-events.jsonl --session-id SESSION_SHA256 --run-id RUN_UUID_HEX` using the event log and identifiers reported for that run. |

Gateway status and the directory-binding check answer different questions. A
healthy service or accepted project identity does not by itself prove that the
current directory is enrolled. Doctor reports the private `event_log` path and
observed events, which can include manual calls. Confirm native agent acceptance
in a fresh session by calling the tools and correlating the receipt with its
event. No event means unknown use, not proven non-use.
Gateway status reports configured limits and expiry, not consumed or remaining
counters or reset timestamps. Current usage requires the gateway ledger or a
usage endpoint supplied by your operator.

There is currently no single aggregate discovery-status command covering every
project and native agent's acceptance. Inspect each project/target with `skills
doctor` and retain native-session evidence separately. These are CLI commands
and JSON reports; discovery has no dedicated terminal user interface (TUI).

Check these independent limits even when preferences say enabled. Provider
limits can preserve local ordering; logging limits affect the evidence recorded:

| Gate | Scope and behavior |
| --- | --- |
| Host attempt ceiling | `--max-provider-calls` limits one host process. Restarting renews only this allowance; one-shot calls start separate hosts. |
| Gateway request count | Shared policy can limit aggregate and per-project requests per day across processes and clients. |
| Gateway spending allowance | Policy can limit aggregate and per-project total spending, plus aggregate and per-project daily spending. A reservation per request can consume allowance even when completion is uncertain. Accounting checks are not a provider billing hard cap. |
| Policy expiry and access | The project grant, allowed data classes and policy expiry remain authoritative. Restarting or changing local preferences cannot renew an expired grant or raise shared limits. |
| Local log capacity | A full or unwritable ledger can produce `log=failed` while discovery still succeeds; this is missing measurement, not a larger provider allowance. |

Use your operator's authoritative policy and free gateway status for configured
limits and expiry, and the operator-supplied usage source for counters; do not
infer these from MetaCTL's host ceiling. Changes to shared budgets or expiry
require operator authorization.
After a denial or error, retain local discovery rather than retrying to bypass a
limit. See [private trials](discovery-trials.md#storage-and-later-analysis) for
ledger capacity and preservation.

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
to `jev-1.13.0`. One call may promote a candidate from the bounded eligible pool
into the returned five. It cannot activate or authorize a skill or bypass catalog
eligibility and fresh digest checks. None, failure, invalid schema, missing key,
exhausted budget or deadline returns the original five in their local ordering. No retries or login
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
you may pay more context/tool-call overhead. Broad default-on distribution as a
context-saving optimization still needs native prompt and task-outcome evidence.
This is separate from an operator's explicit choice to enable advisory Jev
preferences for enrolled projects, described above; that choice does not establish
context savings or hide native skill catalogs.

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

### A globally connected agent reports missing project configuration

A user-wide MCP registration makes discovery available in every workspace. It
does not create a MetaCTL catalog in every folder. When the selected project has
no `metactl.yaml`, discovery reports `project_config_missing` with a routing
receipt showing zero provider calls. This is a local setup failure, not a Jev
rejection or exhausted provider budget. Continue with local skills, configure the
folder with `metactl init --detect`, or supply `--config PATH` for an existing
configuration. Project setup alone does not enroll a project for Jev.

Other catalog failures report `project_discovery_failed`; run
`metactl skills catalog` in the affected folder for local diagnostics. Raw CLI
errors are deliberately kept out of agent responses and discovery logs because
they may include private configuration content.

When logging is enabled, these pre-provider failures are recorded as
`discovery_error` events. Reports count them separately from successful
discoveries; no query, project path, or catalog contents are stored in those
events. Sessions containing only setup errors are excluded from task-outcome
coverage. Status checks remain unlogged, and a failed log write is shown
explicitly in the receipt. Older trial readers that do not recognize
`discovery_error` reject these logs; use the trial reader bundled with the host
version that wrote them or a newer version.

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

## Discovery in folders without project configuration

User-catalog fallback is opt-in. It makes selected local skill libraries available
in explicitly connected folders without creating a project configuration. A
configured project always uses its own catalog, including an empty catalog.
Malformed, unreadable or dangling project configuration remains an error.

Preview a trusted local library and agent target, then save:

~~~sh
metactl skills setup --scope user --source /absolute/path/to/library --target codex-cli
metactl skills setup --scope user --source /absolute/path/to/library --target codex-cli --apply
~~~

The preview shows eligible counts, source visibility and candidate-metadata
classification. Sources must be declared MetaCTL libraries with library.json;
setup does not scan native skill directories, download libraries, or enroll a
workspace for Jev. Relative paths in an existing saved catalog resolve against
the catalog file. The catalog is stored separately from provider preferences in
$XDG_CONFIG_HOME/metactl/discovery-catalog.json, or
~/.config/metactl/discovery-catalog.json, with private file permissions.

The default metadata classification is local-only. If the selected names and
descriptions may leave the device, choose --metadata-policy public-nonsensitive
or --metadata-policy private-owned during setup. Skill bodies remain available
to the coding agent, so choose sources suitable for that agent's visibility
across the connected folders. Classification grants no provider access:
existing exact-workspace enrollment, saved Jev preferences and gateway limits
still apply. Private-owned metadata cannot use a public-only workspace grant.
An unregistered folder reports project_not_enrolled and ranks locally.

Connect an exact fixed workspace without editing an agent configuration:

~~~sh
metactl --project /absolute/workspace --catalog-mode project-or-user skills connect --scope user --target codex-cli --use-preferences
metactl --project /absolute/workspace --catalog-mode project-or-user skills connect --scope user --target codex-cli --use-preferences --apply
metactl --project /absolute/workspace --catalog-mode project-or-user skills doctor --scope user --target codex-cli --use-preferences --json
~~~

User scope currently supports Codex's existing fixed-root registration. It does
**not** follow the folder of every coding session. Use project scope for another
supported adapter and a fixed explicit root. Start a fresh native client session
after applying a connection. Automatic launch-directory discovery, migration of
Git-normalizing external launchers, and native acceptance across unrelated
folders remain separate work; they are not advertised by this release.

For direct discovery, status and loading:

~~~sh
metactl --project /absolute/workspace --catalog-mode project-or-user --discovery-target codex-cli skills catalog --json
metactl --project /absolute/workspace --catalog-mode project-or-user skills host --target codex-cli --runtime codex-cli --use-preferences --status
# The agent supplies minimal JSON to the host's existing --call-tool interface.
metactl --project /absolute/workspace --catalog-mode project-or-user skills host --target codex-cli --runtime codex-cli --use-preferences --call-tool discover_skills
~~~

Status makes no provider call. It separates catalog_ready,
project_config_state, catalog_origin, effective_target,
workspace_resolution and provider_effective_reason. Routine receipts and logs
use opaque context identifiers; local administrative status may show paths.

Persistent off/on controls preserve sources and data policy:

~~~sh
metactl skills setup --disable              # preview, no write
metactl skills setup --disable --apply
metactl skills setup --enable --apply
~~~

Changing sources or classification requires a setup preview and
--replace --apply. A running host rejects changed catalog configuration or
project/user origin with catalog_context_changed; restart it. Ordinary library
content edits are revalidated on each call, and load requires the current
package digest. Save a private copy of the old catalog before replacing it if
you need rollback; restore that copy and restart the host. Provider preferences
and existing logs are separate and remain intact.

## Recommendations are distinct from candidates

Discovery returns eligible result.skills for compatibility. These are candidate
instructions, not automatic activation. The coding agent must still decide
whether a skill is relevant, then load its ID and digest before following it.

| Field value | Meaning |
| --- | --- |
| recommendation_status=recommended | Validated advisory Jev chose the single ID in recommended_ids. |
| recommendation_status=abstained | Validated advisory Jev chose no skill; recommended_ids is empty. Do not activate fallback candidates merely because returned. |
| recommendation_status=ranked_candidates | Local fallback or shadow returned ranked candidates; no provider recommendation is exposed. |
| recommendation_status=no_matches | The deterministic shortlist is empty. |

Shadow mode always exposes only baseline candidate/recommendation behavior, even
when Jev privately abstains. Failures retain local candidates and truthful
provider-attempt accounting. Availability, a successful provider call, or a
changed order does not prove lower coding cost or better outcomes. This workflow
does not suppress the native skill catalog or establish token savings.


Catalog-context-bearing events use metactl.discovery_trial.v2. The bundled
reader and doctor accept both v1 and v2 records in the same ledger; upgrade
readers before inspecting v2 logs. Older strict readers reject v2 explicitly.
Neither version stores queries, skill bodies or workspace paths.
