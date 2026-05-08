# Workflows

## First Success

```bash
cargo build -p metactl
cargo run -p metactl -- demo create --sync
cd "$(cargo run -q -p metactl -- demo path)"
cargo run -p metactl -- --project "$PWD" status
cargo run -p metactl -- --project "$PWD" validate
cargo run -p metactl -- demo destroy --yes
```

## Preview Before Applying

```bash
cargo run -p metactl -- --project /tmp/metactl-demo compile --json
```

Review generated output paths before applying changes into a real repository.

## Brownfield Safety

If a destination file already exists and is not managed by metactl, apply refuses silent takeover. Use preview output to decide whether to copy, patch, symlink, or skip.

## Local MCP Smoke

```bash
cargo run -p metactld -- --mcp --once fixtures/golden/greenfield-claude-code/jsonrpc/search.request.json
```
