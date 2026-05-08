<div align="center">

# metactl

**A local, deterministic control plane for AI coding-agent context.**

Resolve reusable instruction packs against role, policy, and target constraints; compile target-native agent files; then validate what changed before it reaches a repository.

[![CI](https://github.com/pylit-ai/metactl/actions/workflows/ci.yml/badge.svg)](https://github.com/pylit-ai/metactl/actions/workflows/ci.yml)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![Rust](https://img.shields.io/badge/rust-2021-b7410e?logo=rust&logoColor=white)
![API](https://img.shields.io/badge/API-metactl%2Fv2alpha1-2f6f9f)

</div>

`metactl` turns agent instructions into reviewable build artifacts. It knows about roles, packs, policies, and agent targets, so a team can ship one portable context configuration and materialize the right files for Codex CLI, Claude Code, Cursor, Gemini CLI, or OpenClaw.

No hosted service, browser automation, or API key is required for local search, compile, apply, or validation workflows.

The v1 product boundary is fixed by [docs/v1/charter.md](docs/v1/charter.md): a private-by-default deterministic resolver/compiler/validator with 0..N pinned read-only baseline libraries selected by active project/profile, exactly one writable overlay per active profile, and generated project projections.

```bash
metactl --project /tmp/metactl-demo init -t codex-cli --no-input
metactl --project /tmp/metactl-demo compile
metactl --project /tmp/metactl-demo apply --mode copy
metactl --project /tmp/metactl-demo validate
```

Expected success signal:

```text
Initialized /tmp/metactl-demo.

Compiled:
  codex-cli (3 outputs, surface: minimal ...)

Applied:
  codex-cli (copy, 3 files)
    AGENTS.md
    .codex/skills/python-refactor/python-refactor/SKILL.md
    .codex/skills/migration-guard/migration-guard/SKILL.md

Validation:
  codex-cli [pass]
```

## What It Does

- Compiles agent context from explicit `Role`, `Pack`, `Policy`, and `Target` inputs.
- Generates target-native files such as `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursor/rules/*.mdc`, and agent-specific skill folders.
- Keeps generated state under `.metactl/` so output can be reviewed before it is applied.
- Refuses silent takeover of unmanaged brownfield files.
- Previews or applies explicit Fleet Sync across linked local projects from a reviewable controller repo.
- Emits stable JSON for automation with `--json`.
- Exposes the same reference kernel through `metactld` for local stdio JSON-RPC/MCP usage.

## Install From Source

```bash
git clone https://github.com/pylit-ai/metactl.git
cd metactl
cargo install --path crates/metactl --locked
```

Expected success signal:

```text
Installed package `metactl v0.1.0 (...)` (executable `metactl`)
```

For local development without installing:

```bash
cargo run -p metactl -- --help
```

Expected success signal:

```text
Human-first and agent-safe CLI for the metactl kernel

Usage: metactl [OPTIONS] <COMMAND>
```

## Quickstart

Create a clean demo project and materialize Codex CLI context:

```bash
mkdir -p /tmp/metactl-demo
metactl --project /tmp/metactl-demo init -t codex-cli --no-input
metactl --project /tmp/metactl-demo search "release review"
metactl --project /tmp/metactl-demo compile
metactl --project /tmp/metactl-demo apply --mode copy
metactl --project /tmp/metactl-demo validate
```

Expected output includes:

```text
Role:    builder
Policy:  brownfield-safe-builder
Targets: codex-cli
Packs:   python-refactor, migration-guard

Search results for "release review":
- metactl-project-onboarding ...
- migration-guard ...
- python-refactor ...

Validation:
  codex-cli [pass]
```

The demo writes `metactl.yaml`, `metactl.lock.json`, `.metactl/` state, `AGENTS.md`, and Codex skill files into `/tmp/metactl-demo`.

## Daily Workflow

```bash
metactl init -t codex-cli --no-input
metactl list packs
metactl use python-refactor
metactl status
metactl sync
metactl validate
```

Success signal: `status` reports `Execution readiness: ready`, `sync` compiles and applies configured targets, and `validate` reports each target as `[pass]`.

Use `compile` and `apply` separately when you want an explicit review step before files land in the working tree:

```bash
metactl compile --json
metactl apply --mode copy
```

Success signal: `compile --json` returns a compile manifest with `generated_outputs`; `apply` lists every materialized file.

## Fleet Sync

Fleet Sync runs across a controller project that defines `linked_projects`. You can pass the controller explicitly, run from that controller, or store a machine-local pointer:

```bash
metactl fleet controller init personal --path /path/to/metactl-library/fleet/personal
metactl fleet sync --preview
metactl --yes --no-input fleet sync --apply
```

The global pointer is convenience only. The controller repo remains the canonical, reviewable registry. A private metactl library can hold the controller under `fleet/<name>/`; avoid putting it under `packs/`. See [Fleet Sync](docs/user/FLEET_SYNC.md).

## Supported Targets

| Target | Generated surface | Status |
| --- | --- | --- |
| Codex CLI | `AGENTS.md`, `.codex/skills/...` | Tier 1, conformance-covered |
| Claude Code | `CLAUDE.md`, `.claude/skills/...` | Tier 1, conformance-covered |
| Cursor | `AGENTS.md`, `.cursor/rules/*.mdc`, `.cursor/skills/...` | Tier 2, preview |
| Filesystem Agent | `AGENTS.md`, `.metactl/filesystem-agent/...` | Generic compatibility fixture |
| Gemini CLI | `GEMINI.md`, `.gemini/extensions/...` | Tier 2, preview |
| OpenClaw | `OPENCLAW.md` | Target available; compatibility tier not yet claimed |

See [docs/support-matrix.md](docs/support-matrix.md) and [docs/agent-surfaces.md](docs/agent-surfaces.md) for release-specific target notes.

## Dogfooding

This repository tests `metactl` by using it against temporary projects, compiling real target surfaces, applying them, validating them, and checking public-boundary hygiene.

```bash
make verify
```

Expected success signal:

```text
metactl CLI smoke passed
```

Focused checks:

```bash
cargo test -p metactl
cargo check -p metactl -p metactld
bash scripts/smoke_cli.sh
bash scripts/smoke_dogfood.sh
bash scripts/check_public_boundary.sh
```

Expected success signal: every command exits `0`; smoke scripts print their pass line; the boundary check reports no generated local roots, machine paths, or private-only artifacts in the public tree.

## Automation And MCP

`metactl` is designed for local, repo-driven automation. Prefer CLI, JSON, JSON-RPC, or MCP integration over browser automation.

```bash
metactl search "python refactor" --json
metactl validate --json
```

Expected success signal: JSON responses include `api_version: "metactl/v2alpha1"` and either `ok: true` or a stable machine-readable error.

`metactld` exposes the same reference kernel for local stdio JSON-RPC/MCP flows. Start with [docs/mcp/servers.md](docs/mcp/servers.md) and the `run-metactld` Make target when integrating an editor or agent runtime.

## Documentation Map

- [docs/v1/charter.md](docs/v1/charter.md) - canonical v1 scope, vocabulary, and anti-bloat boundary.
- [docs/v1/decisions/private-by-default-sanitized-export.md](docs/v1/decisions/private-by-default-sanitized-export.md) - public decision record for private-by-default sources and explicit sanitized exports.
- [docs/v1/onboarding.md](docs/v1/onboarding.md) - v1 onboarding path for local profile setup, projection preview, and verification.
- [docs/v1/knowledge-sources.md](docs/v1/knowledge-sources.md) - bounded read-only knowledge source manifests and adapter rules.
- [docs/v1/library-stack.md](docs/v1/library-stack.md) - 0..N baseline plus one overlay stack contracts and conflict rules.
- [docs/v1/migration.md](docs/v1/migration.md) - migration path from existing projects and metactlv0-era assumptions.
- [docs/v1/sanitized-export.md](docs/v1/sanitized-export.md) - explicit public export record requirements for private source material.
- [docs/v1/conformance.md](docs/v1/conformance.md) - v1 target conformance matrix and release claim rules.
- [docs/user/GETTING_STARTED.md](docs/user/GETTING_STARTED.md) - first project setup and common commands.
- [docs/user/WORKFLOWS.md](docs/user/WORKFLOWS.md) - preview, brownfield, and local MCP workflows.
- [docs/user/FLEET_SYNC.md](docs/user/FLEET_SYNC.md) - local Fleet controller setup and sync behavior.
- [docs/user/PACK_VISIBILITY.md](docs/user/PACK_VISIBILITY.md) - shared and local pack visibility rules.
- [docs/architecture.md](docs/architecture.md) - reference-kernel model and core nouns.
- [docs/comparisons.md](docs/comparisons.md) - how `metactl` differs from raw agent files, MCP alone, and editor-specific rules.
- [docs/conformance.md](docs/conformance.md) - compatibility claim rules.
- [docs/security-checklist.md](docs/security-checklist.md) and [docs/threat-model.md](docs/threat-model.md) - release and security gates.
- [docs/release-readiness.md](docs/release-readiness.md) - latest local release-readiness record.

## For Coding Agents

Source-of-truth read order:

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
bash scripts/check_public_boundary.sh
```

Public-boundary rules:

- Keep machine-specific paths, credentials, local profiles, and private source names out of public docs, fixtures, and package metadata.
- Do not commit generated local agent roots unless the change intentionally updates public fixtures or starter-library output.
- Review `.metactl/private/` policy reports locally; do not publish private local state.
- Run `scripts/check_public_boundary.sh` before release-facing changes.

## Project Status

Current crate version: `0.1.1`.

Current API version: `metactl/v2alpha1`.

Release-readiness notes are tracked in [docs/release-readiness.md](docs/release-readiness.md). Compatibility statements are release-specific; avoid broad claims such as "certified" unless a release note says so.

## License

Apache-2.0. See [LICENSE](LICENSE).
