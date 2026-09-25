# Agent adapters for optional skill discovery

These examples connect clients to MetaCTL's two-tool, session-bound discovery
host. They do not replace native skill catalogs, change installed skill trees,
or activate retrieved instructions. Start with deterministic baseline mode and
map any host-disabled skills to repeatable `--exclude-skill` arguments. The
[skill-discovery guide](skill-discovery.md) explains eligibility, digest checks,
and provider proof.

## Coverage of standard MetaCTL targets

The canonical inventory is `library/starter/targets/*.json`. Projection support
(generating a client's files) is separate from live discovery acceptance. The
host accepts every canonical target ID as `--runtime`; that label attributes
events and does not install, enable or prove the client's integration.

| Target ID | Discovery connection | Acceptance boundary |
| --- | --- | --- |
| `codex-cli` | Stdio MCP registration below | Check both tools in a fresh native session. Legacy `codex` log label remains accepted. |
| `claude-code` | Stdio MCP; `.mcp.json` or `claude mcp add` | Configuration recipe; live discover/load still required on the installed client. |
| `cursor` | Stdio MCP in project `.cursor/mcp.json` | Configuration recipe; live discover/load still required. |
| `gemini-cli` | Stdio MCP in `.gemini/settings.json` | Configuration recipe; live discover/load still required. |
| `opencode` | Local MCP in `opencode.json` | Experimental target; different envelope described below. |
| `openclaw` | Client/version-specific MCP bridge required | Descriptor advertises MCP, but its shipped runtime template has no server registration schema. No native bridge acceptance is claimed. Use baseline projected instructions until the installed bridge is verified. |
| `filesystem-agent` | Projected instruction files | Descriptor declares no MCP or local-script capability. Automatic discovery is unavailable; an operator can separately run the CLI and attribute it to this label. |

Pi and Omnigent below are additional adapters, not replacements for standard
targets. `other` covers explicitly tested custom clients; `contract` is for
protocol tests. Existing `codex` events are not rewritten or automatically
combined with `codex-cli` cohorts.

### Claude Code, Cursor and Gemini CLI

Generate a baseline server entry with an absolute project and private log path:

```sh
metactl --project /absolute/path/to/project skills host --client-config \
  --ranker deterministic --trial-mode baseline --runtime claude-code \
  --event-log /private/path/discovery-events.jsonl
```

Merge the returned `mcpServers.metactl-skills` entry into the existing client
configuration; preserve other settings. Use `--runtime cursor` or
`--runtime gemini-cli` when generating their entries. Claude also supports:

```sh
claude mcp add --transport stdio --scope project metactl-skills -- \
  metactl --project /absolute/path/to/project skills host \
  --ranker deterministic --trial-mode baseline --runtime claude-code \
  --event-log /private/path/discovery-events.jsonl
```

Review/trust the registration in the client, restart or reload its tools, call
discover then load, and inspect the receipt. Remove only this server entry to
roll back. Formats checked against upstream documentation on 2026-09-25:
[Claude Code](https://code.claude.com/docs/en/mcp),
[Cursor](https://prod.cursor.com/help/customization/mcp),
[Gemini CLI](https://geminicli.com/docs/tools/mcp-server/).

### OpenCode

Generate the same entry with `--runtime opencode`. In `opencode.json`, merge an
entry under `mcp.metactl-skills` with `type: "local"`, `enabled: true`, and a
`command` array containing the generated command followed by every generated
argument. Do not paste the `mcpServers` envelope into OpenCode. See the
[OpenCode local MCP format](https://opencode.ai/docs/mcp-servers/).
Verify both tools on the installed version before declaring native acceptance.

## Codex

Use the installed CLI's stdio registration form. Replace the absolute project
path with a locally configured project and review the flags before registering:

```sh
codex mcp add metactl-skills -- metactl --project /absolute/path/to/project \
  skills host --ranker deterministic --runtime codex-cli --trial-mode baseline \
  --event-log /private/path/discovery-events.jsonl
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
request ceiling. Shutdown closes stdin for graceful host cleanup, then terminates
the remaining POSIX process group after one second. A forced termination can
leave a temporary script directory; it contains code, not credentials. A second
group signal escalates to SIGKILL after 750 milliseconds. The host handles normal
SIGTERM/SIGHUP termination by killing its isolated provider worker; direct
provider workers also enforce a wall-clock alarm. Untrappable host SIGKILL or
machine failure cannot guarantee cleanup of an arbitrary external gateway
command; use a gateway client with its own deadline. The Pi request deadline
is 35 seconds, covering two 15-second CLI calls with margin. On Windows, process-tree cleanup
is not accepted; the trial ledger itself requires POSIX. The Pi contract test
requires an installed package and is intentionally a local optional gate,
not native acceptance from generic CI. Run:

```sh
PI_PACKAGE_ROOT=/absolute/path/to/node_modules/@earendil-works/pi-coding-agent \
  node tests/test_discovery_agent_adapters.mjs
```

The bridge does not automatically retry or renew the per-process Jev
request ceiling. Start a new Pi session only after inspecting and resolving the
failure. Pi's native skill commands remain available.

## Optional gateway evaluation

Only after the project's gateway policy, credential injection, data class, and
per-process request ceiling are approved, add explicit host arguments such as
`--ranker jev --jev-transport gateway --gateway-command /absolute/path/to/jev
--gateway-project PROJECT_ID --gateway-data-class public-nonsensitive --trial-mode shadow
--allow-provider-data --max-provider-calls 4`. Both task queries and candidate
descriptions must be cleared for that route. The `synthetic` classification is
reserved for fixed `--check` requests. Keep keys out of arguments and configuration;
use the approved runtime injector. Shadow mode can record private evaluation
events without changing skill order; advisory mode may reorder a candidate.
Record only an opaque `--session-id`, and keep any `--event-log` path in a private
workspace. The host's deterministic baseline remains usable without gateway
access. A status check is local readiness, a synthetic `--check` validates the
provider path, and real tool metrics show whether a task call used Jev.

These registrations add an external discovery interface. They do not establish
that any client suppressed native skill descriptors before model request
construction; measure that separately before claiming context savings.

For Omnigent Codex sessions choose either the inherited Codex registration or the
inline Omnigent MCP configuration, then verify the live tool list. Enabling both
can duplicate the same tool names.
