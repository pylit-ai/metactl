# Threat Model

`metactl` is a local tool for generating and validating agent-facing project files.

## Assets

- Repository source files.
- Generated agent config and instruction files.
- Public schemas, fixtures, and starter packs.
- Local project state under `.metactl/`.

## Trusted Inputs

- Files already committed to the current repository.
- Public starter-library content in this repository.
- Explicit command-line flags supplied by the user.

## Untrusted Inputs

- Unreviewed packs from arbitrary paths.
- Existing generated files in a brownfield repository.
- Shell commands or hooks embedded in imported content.
- JSON-RPC requests from an untrusted local process.

## Security Goals

- No hidden network calls for local compile, search, validation, or projection.
- Deterministic output for the same inputs.
- Refuse or report unmanaged brownfield conflicts before writing.
- Keep generated local state out of public artifacts.
- Make machine-readable output stable enough for CI checks.

## Non-Goals

- `metactl` is not a sandbox for arbitrary code execution.
- `metactl` does not prove that a pack is safe to run.
- Integrity hashes are not a substitute for signed releases or trusted distribution.
- Local JSON-RPC/MCP stdio is not a remote multi-tenant security boundary.

## Release Gates

- `bash scripts/check_public_boundary.sh`
- `cargo fmt --check`
- `cargo check -p metactl -p metactld`
- `cargo test -p metactl`
- `make verify`
- `cargo package -p metactl --allow-dirty --list`
- `cargo package -p metactld --allow-dirty --list`
- public fixture scan for local paths and local-only content
