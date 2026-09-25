# Skill discovery onboarding proposal

Status: baseline connector and diagnostics implemented on the post-v0.1.23
development branch. Fresh native agent acceptance and Jev default-on rollout
remain separate gates. `skills host --client-config` and `skills trials inspect`
remain available for manual and evaluation paths.

## User problem

A user currently has to distinguish four facts that look like one “enabled”
switch: the project catalog exists; a coding client registered the two-tool
MetaCTL host; an agent actually called discovery; and an optional Jev request
returned validated advice. Task benefit is a fifth, measured outcome. Current
`--client-config` prints generic MCP JSON; clients use different configuration
formats. A status command can report local readiness while the agent never
loaded or called the tool. The user should see these states separately.

## Intended journeys

| Reader | Entry | Happy path | Evidence they need |
| --- | --- | --- | --- |
| First-time user | README and `metactl skills --help` | Select project, connect deterministic discovery, complete a safe demo, learn how to undo | Project ready, client tools visible, baseline receipt, private event |
| Experienced operator | Generated config and target guide | Preview a diff, choose project or user scope, set exclusions/logging, inspect a run | Exact config source, active mode, budget/data policy, event correlation |
| Coding agent | Selected `skill-discovery` pack and optional project instruction | Discover only when specialist instructions help, load ID plus digest, use original instructions | Receipt or explicit skip; source/digest and permission boundaries |
| Evaluator | Private trials guide | Freeze comparable tasks, move baseline → shadow → advisory within approved data and budget | Provider calls, fallbacks, latency, outcome coverage and billing uncertainty |

## Recommended product flow

Keep deterministic discovery available without a provider. Add a target-aware,
preview-first connector after confirming each target's native format:

```text
metactl --project /absolute/project skills connect --target codex-cli --scope project
metactl --project /absolute/project skills connect --target codex-cli --scope project --apply
metactl --project /absolute/project skills doctor --target codex-cli
```

`connect` displays the exact file and entry it will change, the fixed project
path, deterministic baseline mode, a private ledger destination, and a reversible
removal command. `--apply` edits only that server entry and preserves unrelated
configuration values. JSON formatting can change on apply. It never installs an
`AGENTS.md` instruction or switches on paid Jev automatically. Offer a separate,
copyable project instruction after registration. Project config is loaded by
Codex only in trusted projects; the installer must say so.
Removal recognizes the pinned project's MetaCTL baseline entry after a binary
or Python upgrade. Doctor compares the registration with the current invocation.
On a mismatch it reports readiness as unknown and does not execute the
registered command.

`doctor` displays a compact state ladder, with unknown as a first-class
state:

1. **Catalog:** configured project and eligible count from offline status.
2. **Registration:** target config entry found, missing, or conflicting.
3. **Host:** offline readiness checked in the current shell when registration
   matches the requested options, without a provider call.
4. **Agent:** native fresh-session acceptance reported by the user or not yet
   verified; MetaCTL cannot infer it from a config file.
5. **Routing:** last matching event, mode, provider attempts/calls, fallback
   reason, and log status. No event means unknown whether the agent skipped.
6. **Benefit:** task outcome and comparison coverage, only when supplied by an
   evaluator. A health check never fills this field.

The product should label `baseline`, `shadow`, and `advisory` beside the effect
of each mode. Entering shadow/advisory must collect approved data class,
gateway project, a private log, a per-process ceiling, and the gateway's shared
budget. The preview should show that both task text and candidate descriptions
may leave the host. Provider checks must be explicit because they consume a
request. No status command should make a paid call.

## Target mapping

The canonical target inventory has seven IDs. `codex-cli`, `claude-code`,
`cursor`, and `gemini-cli` use distinct MCP configuration formats; `opencode`
uses its local MCP command array. Generate target-native output only for
verified formats. `openclaw` needs a verified version-specific bridge; show a
manual path until one is accepted. `filesystem-agent` has no automatic MCP
integration; offer projected instructions and manual CLI discovery. Pi and
Omnigent are additional adapters. Accepting a runtime label alone is not a
native acceptance test.
The current `skill-discovery` starter pack is projected only for `codex-cli`;
expanding its target list requires target-specific projection and instruction
acceptance tests, not a manifest-only edit.

## Agent instruction placement

Keep the detailed discover → load → use sequence in the optional
`skill-discovery` starter pack. Show a short `AGENTS.md` line in onboarding for
projects that want more reliable invocation. Do not add an unconditional Jev
rule to MetaCTL core instructions: many tasks need no discovery, and Jev may
be unavailable or unauthorized. The server's bootstrap instruction and tool
descriptions help but cannot guarantee that a client calls the tool.

## Acceptance tests before shipping the proposed connector

- A new user with a packaged MetaCTL binary can preview and apply a Codex
  baseline registration to a disposable trusted project without editing JSON
  by hand; unrelated Codex settings are preserved, and rollback is exact.
- A fresh native Codex session exposes both tools and records a baseline
  discovery/load round trip. The display says `provider_calls=0`.
- Missing trust, missing Python, wrong project path, disabled skill mapping,
  unwritable log, and duplicate server entry yield specific, actionable states.
- Each advertised target format has a client-specific integration test; targets
  without an accepted adapter are shown as manual/unsupported, not “enabled.”
- Shadow and advisory are tested only on approved public/synthetic inputs with
  bounded gateway requests; provider failure returns deterministic results.
- A task comparison must measure outcomes, latency, and costs before any
  benefit or default-on claim. Native catalog suppression remains a separate
  acceptance gate.
