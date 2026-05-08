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
target/debug/metactl --project "$PROJECT" compile --json
```

> **Expected JSON shape**
>
> ```json
> {
>   "api_version": "metactl/v2alpha1",
>   "command": "compile",
>   "ok": true,
>   "targets": [
>     {
>       "target": "codex-cli",
>       "outputs": [ ... ]
>     }
>   ]
> }
> ```

Review generated output paths before applying changes into a real repository.

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

## Local MCP Smoke

```bash
make metactl-mcp-smoke
# ok negotiated protocol: 2025-06-18
# ok tools: metactl_search_packs, metactl_explain, metactl_compile_preview, metactl_validate
# ok search first match: metactl-project-onboarding
```
