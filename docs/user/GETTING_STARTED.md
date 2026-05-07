# Getting Started

This guide covers the public local `metactl` CLI.

## Build

```bash
cargo build -p metactl -p metactld
```

## Initialize A Project

```bash
cargo run -p metactl -- --project /path/to/project init --target codex-cli
```

Common target aliases:

| Alias | Target |
| --- | --- |
| `codex` | `codex-cli` |
| `claude` | `claude-code` |
| `cursor` | `cursor` |
| `gemini` | `gemini-cli` |
| `openclaw` | `openclaw` |

## Find And Add Packs

```bash
cargo run -p metactl -- --project /path/to/project search python
cargo run -p metactl -- --project /path/to/project add python-refactor
cargo run -p metactl -- --project /path/to/project sync
```

Use `--json` for automation. Treat JSON output as forward-compatible: rely on documented top-level fields and ignore unknown additions.

## Validate

```bash
cargo run -p metactl -- --project /path/to/project status
cargo run -p metactl -- --project /path/to/project doctor
cargo run -p metactl -- --project /path/to/project validate
```

## Local-Only Packs

`metactl use --local <pack>` writes to `metactl.local.yaml` instead of the shared `metactl.yaml`. Use this for personal experiments or packs that should not be committed.

See `PACK_VISIBILITY.md` for how public and local-only surfaces are separated.

## Fleet Sync

Use Fleet Sync when one controller repo should preview or apply metactl output across several linked local projects:

```bash
cargo run -p metactl -- fleet controller init personal --path /path/to/metactl-library/fleet/personal
cargo run -p metactl -- fleet sync --preview
```

The controller project owns `linked_projects`; the global setting only remembers which controller to use by default. For single-machine setup, omit `--path` and metactl creates `~/.config/metactl/fleet/<name>`. For private metactl libraries, `fleet/<name>/metactl.yaml` is a better home than a loose project under a source checkout parent. See `FLEET_SYNC.md`.
