# metactl

[![CI](https://img.shields.io/github/actions/workflow/status/pylit-ai/metactl/ci.yml?branch=main&label=ci&logo=githubactions&logoColor=white)](https://github.com/pylit-ai/metactl/actions/workflows/ci.yml)
[![metactl on crates.io](https://img.shields.io/crates/v/metactl?label=metactl&logo=rust&logoColor=white)](https://crates.io/crates/metactl)
[![metactld on crates.io](https://img.shields.io/crates/v/metactld?label=metactld&logo=rust&logoColor=white)](https://crates.io/crates/metactld)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![API](https://img.shields.io/badge/API-metactl%2Fv2alpha1-2f6f9f)](#automation-and-mcp)

Current crate version: `0.1.17`

`metactl` is a local control plane for agent instructions. It compiles reusable roles, packs, policies, and targets into reviewable tool-specific files for Codex CLI, Claude Code, Cursor, Gemini CLI, OpenClaw, filesystem agents, and local MCP/JSON-RPC clients.

![metactl quickstart terminal demo](https://raw.githubusercontent.com/pylit-ai/metactl/main/docs/assets/demos/quickstart-hero.gif)

[MP4](docs/assets/demos/quickstart-hero.mp4) / [WebM](docs/assets/demos/quickstart-hero.webm). More terminal walkthroughs live in [docs/cli-demos.md](docs/cli-demos.md). Codex and Claude plugin marketplace export is covered in [docs/user/PLUGIN_MARKETPLACES.md](docs/user/PLUGIN_MARKETPLACES.md).

<!-- TODO: Add public architecture diagram for CLI -> reference kernel -> target adapters once the API surface stabilizes. -->

```bash
metactl demo create --sync
cd "$(metactl demo path)"
metactl sync --adopt patch
metactl validate
metactl demo destroy --yes
```

> **Expected output**
>
> ```text
> Demo sandbox ready: /tmp/.../metactl-demo
> Seed: small brownfield Python repo with an existing AGENTS.md
> Preview sync completed; runtime files were not applied.
> Sync complete.
>   codex-cli [degraded] (patch, surface: full, 72 files)
> ...
> Validation:
>   codex-cli [pass]
> Removed demo sandbox: /tmp/.../metactl-demo
> ```

## Why It Exists

Modern coding agents read different files, directories, skill formats, and rule systems. `metactl` gives a repo one source of truth, then materializes the right surface for each tool without silently taking over unmanaged files.

| Need | metactl behavior |
| --- | --- |
| One canonical agent setup | Compile from explicit `Role`, `Pack`, `Policy`, and `Target` inputs. |
| Review before writing files | Stage output under `.metactl/generated/`, then apply deliberately. |
| Multiple agent targets | Generate `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursor/rules/*.mdc`, skill folders, and generic filesystem surfaces. |
| Brownfield safety | Detect unmanaged files and require explicit handling before overwrite. |
| Automation | Emit stable JSON with `--json` and expose the reference kernel through `metactld`. |
| Local multi-repo operations | Preview Fleet Sync before applying changes across linked projects. |

## Quickstart

Install the CLI from crates.io:

```bash
cargo install metactl --version 0.1.17 --locked
metactl version
# metactl 0.1.17 (metactl/v2alpha1)
```

The published CLI includes the public starter library, so the demo and normal pack workflows do not require a repository checkout or a manual `--starter-library` path.

For a real repo, humans can start with one guided command:

```bash
metactl setup
```

For agents or CI, keep the same flow explicit and non-interactive:

```bash
metactl setup --plan
metactl setup --target codex-cli --yes
metactl ignore fix --plan
```

Run the built-in demo sandbox. It creates a disposable brownfield Python repo with an existing `AGENTS.md`, previews the metactl-generated Codex CLI surface, applies a patch adoption inside that sandbox, validates it, then removes only the sentinel-marked demo directory.

```bash
metactl demo create --sync
cd "$(metactl demo path)"
metactl sync --adopt patch
metactl validate
metactl demo destroy --yes
```

> **Expected output**
>
> ```text
> Demo sandbox ready: /tmp/.../metactl-demo
> Seed: small brownfield Python repo with an existing AGENTS.md
> Preview sync completed; runtime files were not applied.
> Role:    builder
> Policy:  brownfield-safe-builder
> Targets: codex-cli
>
> Validation:
>   codex-cli [pass]
> Removed demo sandbox: /tmp/.../metactl-demo
> ```

No API keys or model-provider credentials are required for this path.

<details>
<summary>Install from a local checkout</summary>

```bash
git clone https://github.com/pylit-ai/metactl.git
cd metactl
cargo install --path crates/metactl --locked
metactl version
# metactl 0.1.17 (metactl/v2alpha1)
```

</details>

<details>
<summary>Install the daemon for JSON-RPC or MCP</summary>

`metactld` exposes the same reference kernel for local stdio JSON-RPC/MCP integration.

```bash
cargo install metactld --version 0.1.17 --locked
metactld --version
# metactld 0.1.17
```

Start with [docs/mcp/servers.md](https://github.com/pylit-ai/metactl/blob/main/docs/mcp/servers.md) when wiring an editor, agent runtime, or local MCP server.

</details>

## Daily Workflow

Use the high-level commands for normal repo work:

```bash
metactl init --detect --no-input
metactl preview
metactl list packs
metactl use python-refactor
metactl status
metactl sync
metactl validate
```

Success signal: `status` reports `Execution readiness: ready`, `sync` compiles and applies configured targets, and `validate` reports each target as `[pass]`.

> **Expected output**
>
> ```text
> Initialized /path/to/project.
> ...
> Resolved "python-refactor" -> pack python-refactor
> Sync complete.
>   codex-cli [ready] (symlink, surface: full, 4 files)
> ...
> Execution readiness: ready
> Validation:
>   codex-cli [pass]
> ```

<details>
<summary>Watch native agent surfaces after sync</summary>

![metactl native agent surfaces demo](https://raw.githubusercontent.com/pylit-ai/metactl/main/docs/assets/demos/agent-native-surfaces.gif)

[MP4](docs/assets/demos/agent-native-surfaces.mp4) / [WebM](docs/assets/demos/agent-native-surfaces.webm)

</details>

Use a two-step review flow when you want to inspect generated files before they land in the working tree:

```bash
metactl compile
metactl apply --mode copy
metactl validate
```

> **Expected output**
>
> ```text
> Project: /path/to/project
> Compiled:
>   codex-cli (4 outputs, surface: full)
> Applied:
>   codex-cli [ready]
> Validation:
>   codex-cli [pass]
> ```

<details>
<summary>Common commands</summary>

| Command | Purpose |
| --- | --- |
| `metactl init --detect` | Detect targets from existing repo surfaces. |
| `metactl init -t codex-cli --no-input` | Explicitly create `metactl.yaml`, `.metactl/`, and a Codex CLI target. |
| `metactl setup` | Human-friendly setup with portable agent artifact stewardship enabled for new projects. |
| `metactl setup --plan` | Show guided setup actions and equivalent raw commands without writing files. |
| `metactl setup --target codex-cli --artifact-policy portable-first --yes` | Create project config for one explicit target without running `sync`. |
| `metactl profile list` | Show user profiles and built-in templates such as `neutral`, `multi-agent`, `agent-ci`, and `solo-codex`. |
| `metactl demo create --sync` | Create a disposable brownfield sandbox and preview generated agent files. |
| `metactl preview` | Convenience alias for `metactl sync --preview`; stages output without applying runtime files. |
| `metactl use <pack>` | Resolve, add, sync, and validate a pack-oriented workflow. |
| `metactl pack use <pack>` | Object-oriented alias for project pack activation; Agent Skill import/export remains under `pack import-skill` and `pack export-skill`. |
| `metactl skills list --scope repo` | Show repo-local Codex skills generated under `.codex/skills`. |
| `metactl skills add <skill-path> --scope user` | Install a Codex skill folder into the user-global `~/.codex/skills` Personal picker source. |
| `metactl add <pack> --sync` | Add a known pack and immediately materialize it. |
| `metactl target add cursor` | Add another target without hand-editing YAML. |
| `metactl explain` | Show why packs and targets were selected. |
| `metactl doctor` | Run local health checks. |
| `metactl revert` | Remove applied outputs tracked by metactl. |
| `metactl ignore install` | Hide generated agent surfaces from local git status. |
| `metactl ignore status` | Check whether generated surfaces and private source state are protected. |
| `metactl ignore fix --plan` | Plan generated-surface ignore repair and Git-index untracking safely. |
| `metactl audit sources` | Diagnose private source cache, lock, and public-example exposure failures. |

</details>

If metactl reports that no starter library is available, first run `metactl doctor`. Use `--starter-library <path>` only for custom/local starter libraries or when troubleshooting a cache materialization failure.

<details>
<summary>Source-audit recovery</summary>

If `sync`, `validate`, `status`, or `doctor` reports private source exposure,
start with the audit command:

```bash
metactl audit sources
```

For local-only private source cache and lock protection, check posture and
install local checkout ignores:

```bash
metactl ignore status
metactl ignore fix --plan --scope local --include-private-sources
metactl ignore fix --scope local --include-private-sources --yes
```

Local ignore scope writes `.git/info/exclude` for the current checkout only. Use
repo scope only when the team wants the ignore posture committed. If generated
roots are already tracked, use `--untrack-generated --yes`; this removes them
from the Git index only and leaves files on disk.

</details>

<details>
<summary>Brownfield projects</summary>

For existing repositories, prefer preview first:

```bash
metactl init --detect --no-input
metactl preview
metactl validate
```

> **Expected output**
>
> ```text
> Initialized /path/to/project.
> Detected targets: codex-cli
> Sync complete.
>   codex-cli [ready] (symlink, surface: full, ...)
> Preview only; runtime files were not changed.
> Validation:
>   codex-cli [warn]
>     warn No managed state found for target codex-cli.
> ```

Then apply only after reviewing `.metactl/generated/`:

```bash
metactl apply --mode copy
metactl validate
```

> **Expected output**
>
> ```text
> Applied:
>   codex-cli [ready]
> Validation:
>   codex-cli [pass]
> ```

`metactl` is intentionally conservative around unmanaged files. If a target file already exists and is not tracked by metactl, expect a reviewable conflict rather than silent overwrite.

</details>

## Demo Sandbox

Use `metactl demo` when you want to try brownfield behavior without touching a real repo:

```bash
metactl demo create --sync
metactl demo list
metactl demo path
metactl demo reset --yes
metactl demo destroy --yes
```

> **Expected output**
>
> ```text
> Demo sandbox ready: /tmp/.../metactl-demo
> Seed: small brownfield Python repo with an existing AGENTS.md
> Target: codex-cli
> Preview sync completed; runtime files were not applied.
> Demo sandboxes under ...
> Removed demo sandbox: /tmp/.../metactl-demo
> ```

`demo destroy` and `demo reset` verify a `.metactl-demo/manifest.json` sentinel before removing files.

<details>
<summary>Demo commands</summary>

| Command | Purpose |
| --- | --- |
| `metactl demo create --sync` | Create the sandbox and run a preview sync. |
| `metactl demo list` | List demo sandboxes under the metactl demo home. |
| `metactl demo path` | Print the sandbox path for shell navigation. |
| `metactl demo reset --yes` | Recreate a sentinel-marked sandbox from scratch. |
| `metactl demo destroy --yes` | Remove a sentinel-marked sandbox. |

Set `METACTL_DEMO_HOME` to isolate demos in CI or temporary test runs.

</details>

## Supported Targets

| Target | Generated surface | Status |
| --- | --- | --- |
| Codex CLI | `AGENTS.md`, `.codex/skills/...` | Tier 1, conformance-covered. Repo-local skills are visible to Codex sessions opened in that repo; user-global Personal skills live under `~/.codex/skills`. |
| Claude Code | `CLAUDE.md`, `.claude/skills/...` | Tier 1, conformance-covered |
| Cursor | `AGENTS.md`, `.cursor/rules/*.mdc`, `.cursor/skills/...` | Tier 2, preview |
| Filesystem Agent | `AGENTS.md`, `.metactl/filesystem-agent/...` | Generic compatibility fixture |
| Gemini CLI | `GEMINI.md`, `.gemini/extensions/...` | Tier 2, preview |
| OpenClaw | `OPENCLAW.md` | Target available; compatibility tier not yet claimed |

See [docs/support-matrix.md](https://github.com/pylit-ai/metactl/blob/main/docs/support-matrix.md) and [docs/agent-surfaces.md](https://github.com/pylit-ai/metactl/blob/main/docs/agent-surfaces.md) for release-specific target notes.

## Dogfooding

`metactl` ships with repo-local checks that exercise the same surfaces it generates for users:

```bash
make smoke-dogfood
make metactl-validate-contracts
make metactl-surface-benchmark
scripts/check_public_boundary.sh
```

Expected result: dogfood, contract, and surface benchmark checks pass, and the public boundary scanner reports no private-source leaks. The surface benchmark proves local projection budget and route retention; it does not claim provider-backed model task success.

> **Expected output**
>
> ```text
> metactl dogfood smoke passed
> validated: fixtures/library/evals/activation-trace.sample.json
> ...
> contracts: OK
> Public boundary OK
> ```

<details>
<summary>What dogfooding covers</summary>

- CLI workflow smoke tests through `scripts/smoke_cli.sh`.
- Stdio daemon smoke tests through `scripts/smoke_stdio.sh`.
- Trust-blindspot regression coverage through `scripts/smoke_trust_blindspots.sh`.
- Public/private boundary checks through `scripts/check_public_boundary.sh`.
- Contract validation through `make metactl-validate-contracts`.

</details>

## Fleet Sync

Fleet Sync previews or applies explicit sync across linked local projects from a reviewable controller repo:

```bash
metactl fleet status
metactl fleet sync --preview
```

Expected result: `status` reports linked project readiness, and `sync --preview` shows planned project updates without applying them.

Fleet Sync updates repo-local generated surfaces in linked projects. It does not install Codex skills into the user-global Personal picker source. Use `metactl skills add <repo-skill-path> --scope user` when an operator-facing skill should also appear under `~/.codex/skills`.

> **Expected output**
>
> ```text
> Fleet controller: team-agents
> Controller source: user_default
> Controller path: /path/to/team-agents
> Fleet sync preview:
>   /path/to/project [ready]
> ```

<details>
<summary>Controller setup</summary>

Create a controller, select it, and link projects deliberately:

```bash
metactl fleet controller init team-agents
metactl fleet controller set team-agents /path/to/team-agents
metactl project link /path/to/project
metactl fleet status
```

> **Expected output**
>
> ```text
> Fleet controller `team-agents` initialized at /path/to/team-agents.
> Next: edit /path/to/team-agents/metactl.yaml and add linked_projects, then run `metactl fleet sync --preview`.
> Fleet controller: team-agents
> Controller source: user_default
> Controller path: /path/to/team-agents
> ```

See [docs/user/FLEET_SYNC.md](https://github.com/pylit-ai/metactl/blob/main/docs/user/FLEET_SYNC.md) for controller layout, sync behavior, preview/apply semantics, and failure modes.

</details>

## Automation And MCP

`metactl` is designed for local, repo-driven automation. Prefer CLI, JSON, JSON-RPC, or MCP integration over browser automation.

```bash
PROJECT="$(mktemp -d /tmp/metactl-json.XXXXXX)"
metactl --project "$PROJECT" --agent setup --plan --target codex-cli
metactl --project "$PROJECT" setup --target codex-cli --yes
metactl --project "$PROJECT" use python-refactor
metactl --project "$PROJECT" --agent status
metactl --project "$PROJECT" --agent validate
```

> **Expected JSON shape**
>
> ```json
> {
>   "api_version": "metactl/v2alpha1",
>   "ok": true
> }
> ```

<details>
<summary>Local MCP and JSON-RPC entrypoints</summary>

- `metactld` runs the local reference kernel for stdio JSON-RPC/MCP flows.
- `make run-metactld` starts the daemon from a checkout.
- `make metactl-mcp-smoke` runs the MCP smoke path.
- [docs/mcp/servers.md](https://github.com/pylit-ai/metactl/blob/main/docs/mcp/servers.md) covers server setup and integration notes.

</details>

<details>
<summary>Security and privacy posture</summary>

- `metactl` is local-first. It does not require model-provider credentials for compile, apply, status, or validation.
- Private source material must stay in private pack sources unless exported through an explicit sanitized export record.
- `metactl audit` and `metactl check-public-boundary` help identify source leaks and boundary mistakes.
- Generated adapter state belongs under `.metactl/` and target-specific generated surfaces, not in package metadata or public examples by accident.

See [docs/security-checklist.md](https://github.com/pylit-ai/metactl/blob/main/docs/security-checklist.md), [docs/threat-model.md](https://github.com/pylit-ai/metactl/blob/main/docs/threat-model.md), and [docs/v1/sanitized-export.md](https://github.com/pylit-ai/metactl/blob/main/docs/v1/sanitized-export.md).

</details>

## Documentation

| Reader | Start here |
| --- | --- |
| New user | [docs/user/GETTING_STARTED.md](https://github.com/pylit-ai/metactl/blob/main/docs/user/GETTING_STARTED.md) |
| Demo viewer | [docs/cli-demos.md](https://github.com/pylit-ai/metactl/blob/main/docs/cli-demos.md) |
| Daily operator | [docs/user/WORKFLOWS.md](https://github.com/pylit-ai/metactl/blob/main/docs/user/WORKFLOWS.md) |
| Fleet operator | [docs/user/FLEET_SYNC.md](https://github.com/pylit-ai/metactl/blob/main/docs/user/FLEET_SYNC.md) |
| Pack author | [docs/user/PACK_VISIBILITY.md](https://github.com/pylit-ai/metactl/blob/main/docs/user/PACK_VISIBILITY.md) |
| Integrator | [docs/mcp/servers.md](https://github.com/pylit-ai/metactl/blob/main/docs/mcp/servers.md) |
| Maintainer | [docs/release-readiness.md](https://github.com/pylit-ai/metactl/blob/main/docs/release-readiness.md) |
| Reviewer | [docs/v1/charter.md](https://github.com/pylit-ai/metactl/blob/main/docs/v1/charter.md) |

<details>
<summary>Reference docs</summary>

- [docs/architecture.md](https://github.com/pylit-ai/metactl/blob/main/docs/architecture.md) - reference kernel, projection model, and target adapters.
- [docs/conformance.md](https://github.com/pylit-ai/metactl/blob/main/docs/conformance.md) - conformance expectations and target checks.
- [docs/comparisons.md](https://github.com/pylit-ai/metactl/blob/main/docs/comparisons.md) - comparison notes against adjacent tools.
- [docs/v1/onboarding.md](https://github.com/pylit-ai/metactl/blob/main/docs/v1/onboarding.md) - v1 onboarding path for local profile setup, projection preview, and verification.
- [docs/v1/knowledge-sources.md](https://github.com/pylit-ai/metactl/blob/main/docs/v1/knowledge-sources.md) - bounded read-only knowledge source manifests and adapter rules.
- [docs/v1/library-stack.md](https://github.com/pylit-ai/metactl/blob/main/docs/v1/library-stack.md) - 0..N baseline plus one overlay stack contracts and conflict rules.
- [docs/v1/private-by-default-sanitized-export decision](https://github.com/pylit-ai/metactl/blob/main/docs/v1/decisions/private-by-default-sanitized-export.md) - public decision record for private-by-default sources and explicit sanitized exports.

</details>

## For Coding Agents

Public-source read order:

1. `README.md`
2. `Cargo.toml` and `crates/*/Cargo.toml`
3. `docs/user/GETTING_STARTED.md`
4. `docs/architecture.md`
5. `docs/security-checklist.md`

Safe verification commands:

```bash
cargo fmt --check
cargo test -p metactl
cargo check -p metactl -p metactld
make metactl-validate-contracts
scripts/check_public_boundary.sh
```

> **Expected output**
>
> ```text
> test result: ok. ...
>     Finished `dev` profile ...
> validated: fixtures/library/evals/activation-trace.sample.json
> ...
> Public boundary OK
> ```

Boundary rules:

- Keep private pack sources, account names, customer details, secrets, and local adapter output out of the public repo.
- Prefer generic paths such as `/path/to/project` in public docs.
- Use explicit sanitized export records for public examples derived from private sources.
- Do not treat generated files as hand-authored source unless a command or doc says they are meant to be checked in.

<details>
<summary>Development gates</summary>

```bash
make verify-docs-links
make verify-docs-commands
make smoke-cli
make smoke-stdio
make verify
```

> **Expected output**
>
> ```text
> verify-docs-links: OK
> verify-docs-commands: OK
> metactl CLI smoke passed
> metactl stdio smoke passed
> metactl dogfood smoke passed
> ```

Use the smallest focused gate for a local edit, then broaden to `make verify` before release-sensitive changes.

</details>

## Project Status

Current public crate version: `0.1.17` for both `metactl` and `metactld`.

`metactl` is ready for local CLI workflows, sentinel-guarded demo sandboxes, Codex CLI and Claude Code targets, conformance-covered packaging, and local automation through JSON/JSON-RPC/MCP. Some target adapters and Fleet Sync workflows are intentionally marked preview until their support matrix entries are promoted.

## License

Apache-2.0. See [LICENSE](LICENSE).
