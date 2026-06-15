# Getting Started

This guide covers the public local `metactl` CLI.

## Install

Install the CLI from crates.io:

```bash
cargo install metactl --version 0.1.18 --locked
# Installed package `metactl v0.1.18` (executable `metactl`)
```

The published `metactl` package bundles the public starter library. You do not need a checkout of this repository for the built-in demo, `metactl list packs`, or the default `python-refactor` workflow.

Install `metactld` only if you need the local JSON-RPC/MCP daemon:

```bash
cargo install metactld --version 0.1.18 --locked
# Installed package `metactld v0.1.18` (executable `metactld`)
```

The pinned commands above reproduce this release. To update to the latest
published crates.io versions later:

```bash
cargo install metactl --locked --force
cargo install metactld --locked --force
# Installed package `metactl v0.1.18` (executable `metactl`)
# Installed package `metactld v0.1.18` (executable `metactld`)
```

Check installed binaries:

```bash
metactl --version
metactld --version
# metactl 0.1.18 (metactl/v2alpha1)
# metactld 0.1.18
```

For source development:

```bash
git clone https://github.com/pylit-ai/metactl.git
cd metactl
cargo build -p metactl -p metactld
#    Compiling metactl v0.1.18 (...)
#    Compiling metactld v0.1.18 (...)
#     Finished `dev` profile ...
```

## Initialize A Project

To try metactl without touching an existing repo, create a disposable brownfield sandbox:

```bash
metactl demo create --sync
cd "$(metactl demo path)"
metactl status
metactl sync --adopt patch
metactl validate
```

Use `--starter-library <path>` only when pointing metactl at a custom/local library or when diagnosing a starter-library cache materialization failure.

> **Expected output**
>
> ```text
> Demo sandbox ready: /tmp/.../metactl-demo
> Seed: small brownfield Python repo with an existing AGENTS.md
> Preview sync completed; runtime files were not applied.
> ...
> Execution readiness: ready
> Sync complete.
>   codex-cli [degraded] (patch, surface: full, 72 files)
> Validation:
>   codex-cli [pass]
> ```

When done, remove the sandbox and its generated files:

```bash
metactl demo destroy --yes
# Removed demo sandbox: /tmp/.../metactl-demo
```

`demo destroy` only removes directories with a `.metactl-demo/manifest.json`
sentinel created by metactl.

For a real repo, humans can use one guided setup command:

```bash
metactl --project /path/to/project setup
```

`setup` enables portable agent artifact stewardship for new projects, so
reusable skills, rules, commands, prompts, and workflows route through metactl
instead of living only in one agent's native folder.

`--plan` is read-only and shows the raw equivalent commands that automation can
run later.

```bash
metactl --project /path/to/project setup --plan
metactl --project /path/to/project setup --target codex-cli --yes
metactl --project /path/to/project ignore fix --plan
```

`setup --plan` includes a report-only background refresh recommendation by
default. Persistent OS scheduler installation is explicit:

```bash
metactl --project /path/to/project background plan --scope project
metactl --project /path/to/project background install --scope project --yes
metactl --project /path/to/project background status --scope project
```

To make setup perform that install in one confirmed flow:

```bash
metactl --project /path/to/project setup --target codex-cli --install-background --yes
```

Use `setup --no-background` when the project should not recommend a scheduled
refresh.

For agent or CI use, keep the same flow non-interactive:

```bash
metactl --project /path/to/project --agent setup --plan --target codex-cli
metactl --project /path/to/project setup --target codex-cli --artifact-policy portable-first --yes
metactl --project /path/to/project background install --scope project --yes
metactl --project /path/to/project --agent ignore fix --plan
```

```bash
metactl --project /path/to/project init --target codex-cli
```

> **Expected output**
>
> ```text
> Initialized /path/to/project.
>
>   Config:  metactl.yaml
>   Role:    builder
>   Policy:  brownfield-safe-builder
>   Targets: codex-cli
>
> Next steps:
>   metactl use python-refactor    Activate a pack (resolve + add + sync)
> ```

Common target aliases:

| Alias | Target |
| --- | --- |
| `codex` | `codex-cli` |
| `claude` | `claude-code` |
| `cursor` | `cursor` |
| `gemini` | `gemini-cli` |
| `openclaw` | `openclaw` |

Public `init` is target-neutral unless a target is explicit, detected from existing repo surfaces, or selected by a profile:

```bash
metactl --project /path/to/project init --detect --no-input
metactl --project /path/to/project profile list
metactl --project /path/to/project profile set-default solo-codex
```

Built-in profile templates are public and do not require private library paths. Use `neutral` for explicit target selection, `multi-agent` for several runtimes, `agent-ci` for automation posture, and `solo-codex` only when Codex CLI is an intentional target choice.

## Find And Add Packs

```bash
metactl --project /path/to/project search python
metactl --project /path/to/project pack add python-refactor
metactl --project /path/to/project sync
```

> **Expected output**
>
> ```text
> Matches:
>   python-refactor  Python Refactor
> Added pack python-refactor to metactl.yaml.
> Sync complete.
>   codex-cli [ready] (symlink, surface: full, 4 files)
> ```

Use `--json` for automation. Treat JSON output as forward-compatible: rely on documented top-level fields and ignore unknown additions.

Use `--agent` when an automation runner needs JSON, no prompts, and stable recoverable-error fields:

```bash
metactl --project /path/to/project --agent status
metactl --project /path/to/project --agent validate
```

## Validate

```bash
metactl --project /path/to/project status
metactl --project /path/to/project doctor
metactl --project /path/to/project validate
```

> **Expected output**
>
> ```text
> Execution readiness: ready
> Doctor:
>   [pass] config
>   [pass] lock
> Validation:
>   codex-cli [pass]
> ```

## Local-Only Packs

`metactl use --local <pack>` writes to `metactl.local.yaml` instead of the shared `metactl.yaml`. Use this for personal experiments or packs that should not be committed.

See `PACK_VISIBILITY.md` for how public and local-only surfaces are separated.

## Fleet Sync

Use Fleet Sync when one controller repo should preview or apply metactl output across several linked local projects:

```bash
metactl fleet controller init personal --path /path/to/metactl-library/fleet/personal
metactl fleet sync --preview
```

> **Expected output**
>
> ```text
> Fleet controller `personal` initialized at /path/to/metactl-library/fleet/personal.
> Next: edit /path/to/metactl-library/fleet/personal/metactl.yaml and add linked_projects, then run `metactl fleet sync --preview`.
> Fleet sync preview:
>   /path/to/project [ready]
> ```

The controller project owns `linked_projects`; the global setting only remembers which controller to use by default. For single-machine setup, omit `--path` and metactl creates `~/.config/metactl/fleet/<name>`. For private metactl libraries, `fleet/<name>/metactl.yaml` is a better home than a loose project under a source checkout parent. See `FLEET_SYNC.md`.
