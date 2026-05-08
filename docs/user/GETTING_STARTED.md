# Getting Started

This guide covers the public local `metactl` CLI.

## Install

Install the CLI from crates.io:

```bash
cargo install metactl --version 0.1.4 --locked
```

Expected success signal:

```text
Installed package `metactl v0.1.4` (executable `metactl`)
```

Install `metactld` only if you need the local JSON-RPC/MCP daemon:

```bash
cargo install metactld --version 0.1.4 --locked
```

**Expected output:**

```text
Installed package `metactld v0.1.4` (executable `metactld`)
```

Expected success signal:

```text
Installed package `metactld v0.1.4` (executable `metactld`)
```

The pinned commands above reproduce this release. To update to the latest
published crates.io versions later:

```bash
cargo install metactl --locked --force
cargo install metactld --locked --force
```

**Expected output:**

```text
Installed package `metactl v0.1.4` (executable `metactl`)
Installed package `metactld v0.1.4` (executable `metactld`)
```

Check installed binaries:

```bash
metactl --version
metactld --version
```

**Expected output:**

```text
metactl 0.1.4 (metactl/v2alpha1)
metactld 0.1.4
```

For source development:

```bash
git clone https://github.com/pylit-ai/metactl.git
cd metactl
cargo build -p metactl -p metactld
```

**Expected output:**

```text
   Compiling metactl v0.1.4 (...)
   Compiling metactld v0.1.4 (...)
    Finished `dev` profile ...
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

**Expected output:**

```text
Demo sandbox ready: /tmp/.../metactl-demo
Seed: small brownfield Python repo with an existing AGENTS.md
Preview sync completed; runtime files were not applied.
...
Execution readiness: ready
Sync complete.
  codex-cli [degraded] (patch, surface: full, 72 files)
Validation:
  codex-cli [pass]
```

When done, remove the sandbox and its generated files:

```bash
metactl demo destroy --yes
```

**Expected output:**

```text
Removed demo sandbox: /tmp/.../metactl-demo
```

`demo destroy` only removes directories with a `.metactl-demo/manifest.json`
sentinel created by metactl.

```bash
metactl --project /path/to/project init --target codex-cli
```

**Expected output:**

```text
Initialized /path/to/project.

  Config:  metactl.yaml
  Role:    builder
  Policy:  brownfield-safe-builder
  Targets: codex-cli

Next steps:
  metactl use python-refactor    Activate a pack (resolve + add + sync)
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
metactl --project /path/to/project search python
metactl --project /path/to/project add python-refactor
metactl --project /path/to/project sync
```

**Expected output:**

```text
Matches:
  python-refactor  Python Refactor
Added pack python-refactor to metactl.yaml.
Sync complete.
  codex-cli [ready] (symlink, surface: full, 4 files)
```

Use `--json` for automation. Treat JSON output as forward-compatible: rely on documented top-level fields and ignore unknown additions.

## Validate

```bash
metactl --project /path/to/project status
metactl --project /path/to/project doctor
metactl --project /path/to/project validate
```

**Expected output:**

```text
Execution readiness: ready
Doctor:
  [pass] config
  [pass] lock
Validation:
  codex-cli [pass]
```

## Local-Only Packs

`metactl use --local <pack>` writes to `metactl.local.yaml` instead of the shared `metactl.yaml`. Use this for personal experiments or packs that should not be committed.

See `PACK_VISIBILITY.md` for how public and local-only surfaces are separated.

## Fleet Sync

Use Fleet Sync when one controller repo should preview or apply metactl output across several linked local projects:

```bash
metactl fleet controller init personal --path /path/to/metactl-library/fleet/personal
metactl fleet sync --preview
```

**Expected output:**

```text
Fleet controller `personal` initialized at /path/to/metactl-library/fleet/personal.
Next: edit /path/to/metactl-library/fleet/personal/metactl.yaml and add linked_projects, then run `metactl fleet sync --preview`.
Fleet sync preview:
  /path/to/project [ready]
```

The controller project owns `linked_projects`; the global setting only remembers which controller to use by default. For single-machine setup, omit `--path` and metactl creates `~/.config/metactl/fleet/<name>`. For private metactl libraries, `fleet/<name>/metactl.yaml` is a better home than a loose project under a source checkout parent. See `FLEET_SYNC.md`.
