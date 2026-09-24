# Agent adapters for optional skill discovery

These examples connect clients to MetaCTL's two-tool, session-bound discovery
host. They do not replace native skill catalogs, change installed skill trees,
or activate retrieved instructions. Start with deterministic baseline mode and
map any host-disabled skills to repeatable `--exclude-skill` arguments. The
[skill-discovery guide](skill-discovery.md) explains eligibility, digest checks,
and provider proof.

## Codex

Use the installed CLI's stdio registration form. Replace the absolute project
path with a locally configured project and review the flags before registering:

```sh
codex mcp add metactl-skills -- metactl --project /absolute/path/to/project \
  skills host --ranker deterministic --runtime codex --trial-mode baseline
codex mcp get metactl-skills
```

Start a fresh Codex session and inspect its available tools for
`discover_skills` and `load_skill`. Ask it to discover a harmless ambiguous task,
then load one returned ID and digest. Confirm the returned metrics and unchanged
native skill catalog. `codex mcp remove metactl-skills` reverses this registration.
Omnigent Codex native sessions copy the user's Codex configuration at launch;
restart such a session to see a changed registration. The copied session config
and live tools should be checked separately.

## Omnigent

[`omnigent-agent.yaml`](../../examples/skill-discovery/omnigent-agent.yaml) is an
installed-parser-compatible example for an Omnigent agent root. Copy it as that
agent's `config.yaml`, replace the project path, and retain existing agent
configuration when applying it to a real agent. The inline `tools.metactl_skills`
entry uses Omnigent's `type: mcp`, `command`, and literal `args` schema. Omnigent
does not expand environment variables in `args`; use an exact project path.
Alternatively, an agent-root `tools/mcp/metactl-skills.yaml` can declare `name`,
`transport: stdio`, `command`, and `args`. Validate the agent spec with the
installed Omnigent parser, then launch a fresh agent and call both tools. Check
that the agent can still use its pre-existing tools and skills. This example
does not claim a native runtime acceptance result.

## Pi

Pi has no core MCP registration. [`pi-extension.ts`](../../examples/skill-discovery/pi-extension.ts)
registers the same two names with Pi's extension API and bridges them to one
persistent MCP child. The fixed command and argument array come only from the
operator's environment. `METACTL_DISCOVERY_ARGS_JSON` is required; there is no
implicit project, provider, or paid configuration. For a baseline local run:

```sh
export METACTL_DISCOVERY_COMMAND=metactl
export METACTL_DISCOVERY_ARGS_JSON='["--project","/absolute/path/to/project","skills","host","--ranker","deterministic","--runtime","pi","--trial-mode","baseline"]'
pi -e /absolute/path/to/metactl/examples/skill-discovery/pi-extension.ts
```

Load the extension with `-e` for one session or install it in a trusted Pi
project's `.pi/extensions/` directory after review. A Pi tool call first starts
the host. The bridge serializes calls, bounds request time and output, and
closes the child at session shutdown. On timeout, cancellation, process exit,
malformed response, or host error it fails closed for the rest of that Pi
session. It does **not** restart a child and silently renew its per-process Jev
request ceiling. Start a new Pi session only after inspecting and resolving the
failure. Pi's native skill commands remain available.

## Optional gateway evaluation

Only after the project's gateway policy, credential injection, data class, and
per-process request ceiling are approved, add explicit host arguments such as
`--ranker jev --jev-transport gateway --gateway-command /absolute/path/to/jev
--gateway-project PROJECT_ID --gateway-data-class synthetic --trial-mode shadow
--allow-provider-data --max-provider-calls 20`. Use `public-nonsensitive` only
for data cleared for that route. Keep keys out of arguments and configuration;
use the approved runtime injector. Shadow mode can record private evaluation
events without changing skill order; advisory mode may reorder a candidate.
Record only an opaque `--session-id`, and keep any `--event-log` path in a private
workspace. The host's deterministic baseline remains usable without gateway
access. A status check is local readiness, a synthetic `--check` validates the
provider path, and real tool metrics show whether a task call used Jev.

These registrations add an external discovery interface. They do not establish
that any client suppressed native skill descriptors before model request
construction; measure that separately before claiming context savings.
