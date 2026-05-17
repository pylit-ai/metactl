# Workflows

## First Success

```bash
cargo build -p metactl
METACTL="$(pwd)/target/debug/metactl"
"$METACTL" demo create --sync
cd "$("$METACTL" demo path)"
"$METACTL" --project "$PWD" status
"$METACTL" --project "$PWD" sync --adopt patch
"$METACTL" --project "$PWD" validate
cd -
"$METACTL" demo destroy --yes
```

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
> Removed demo sandbox: /tmp/.../metactl-demo
> ```

## Preview Before Applying

```bash
PROJECT="$(mktemp -d /tmp/metactl-preview.XXXXXX)"
target/debug/metactl --project "$PROJECT" init -t codex-cli --no-input
target/debug/metactl --project "$PROJECT" preview --json
```

> **Expected JSON shape**
>
> ```json
> {
>   "api_version": "metactl/v2alpha1",
>   "command": "sync",
>   "ok": true,
>   "preview": true,
>   "targets": [ ... ]
> }
> ```

Review generated output paths before applying changes into a real repository.

## Command Defaults And Agent Mode

Object groups default to safe read-only views when no subcommand is provided:

```bash
metactl target
metactl source
metactl profile
metactl ignore
metactl audit
metactl fleet
metactl demo
```

For non-interactive runners, `--agent` implies JSON and no prompts:

```bash
metactl --agent status
metactl --agent validate
```

Project pack activation has both top-level and object-oriented aliases:

```bash
metactl use python-refactor
metactl pack use python-refactor
metactl pack add unit-test-loop
metactl pack remove unit-test-loop
```

## Brownfield Safety

If a destination file already exists and is not managed by metactl, apply refuses silent takeover. Use preview output to decide whether to copy, patch, symlink, or skip.

> **Expected refusal output**
>
> ```text
> Error: Apply refused for target codex-cli.
>   - AGENTS.md: Unmanaged destination exists and metactl refused silent takeover.
>   - Next: metactl sync --adopt preview
>   - Next: metactl sync --adopt patch
>   - Next: metactl sync --adopt takeover
> ```

## Source-Audit Recovery

When `sync`, `validate`, `status`, or `doctor` reports private source exposure,
inspect the concrete findings first:

```bash
metactl audit sources
metactl source
metactl source sync
```

If `metactl ignore status` reports `private-sources not-protected`, protect this
checkout's private source cache and lock state:

```bash
metactl ignore status
metactl ignore install --scope local --include-private-sources
```

Local scope writes `.git/info/exclude` and affects only the current checkout.
Use `--scope repo` only when the ignore posture should be shared by the repo.

`metactl source add <LOCATION>` infers a source id from `library.json` when present, or from the basename when unambiguous. Use `metactl source add <NAME> <LOCATION>` for scripts that need explicit names.

## Local MCP Smoke

```bash
make metactl-mcp-smoke
# ok negotiated protocol: 2025-06-18
# ok tools: metactl_search_packs, metactl_explain, metactl_compile_preview, metactl_validate
# ok search first match: metactl-project-onboarding
```
