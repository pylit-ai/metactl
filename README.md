# metactl

`metactl` is a local deterministic context control plane for AI coding agents. It keeps agent behavior source-controlled, explainable, portable, and target-native without coupling behavior to one editor or agent runtime.

## At A Glance

- **Core model:** `Role`, `Pack`, `Policy`, `Target`
- **Primary binary:** `metactl`
- **Local service shim:** `metactld` for stdio JSON-RPC/MCP usage
- **Public starter library:** `library/starter`
- **Stable machine-readable contracts:** `contracts/`
- **Verification fixtures:** `fixtures/`

## Quick Start

```bash
cargo build -p metactl -p metactld
cargo run -p metactl -- --help
cargo run -p metactl -- init --target codex-cli
cargo run -p metactl -- search python --json
```

Run commands from the repo root when using `cargo run`. Pass `--project <path>` to operate on another repository.

## Repository Map

| Path | Purpose |
| --- | --- |
| `crates/metactl/` | Local CLI and library crate |
| `crates/metactld/` | Local stdio JSON-RPC/MCP shim |
| `contracts/` | Public schemas and JSON-RPC method contracts |
| `fixtures/` | Public verification fixtures |
| `library/starter/` | Small public example library |
| `docs/user/` | User-facing CLI docs |
| `docs/mcp/` | Local MCP setup notes |
| `docs/architecture.md` | Public architecture overview |
| `docs/threat-model.md` | Public security model and non-goals |
| `docs/support-matrix.md` | Adapter support tiers |
| `docs/conformance.md` | Compatibility badge rules |
| `docs/comparisons.md` | Positioning against single-agent files and MCP alone |
| `docs/release-readiness.md` | Latest release gate and dependency scan record |

## Verification

```bash
cargo fmt --check
cargo test -p metactl
cargo check -p metactl -p metactld
make metactl-validate-contracts
bash scripts/check_public_boundary.sh
```

For the broader local smoke suite:

```bash
make verify
```

## Repository Hygiene

Keep generated local agent config, machine-specific paths, local notes, and environment-specific files out of committed examples and release artifacts.

## License

Code, public schemas, fixtures, and starter examples are licensed under Apache-2.0. Documentation is published under CC-BY-4.0 unless a file says otherwise. See `LICENSE-DOCS`.
