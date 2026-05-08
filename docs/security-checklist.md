# Security Checklist

Run this checklist before a public release.

## Boundary

- Public tree contains no machine-specific paths.
- Public tree contains no generated local agent roots.
- Public examples use neutral placeholders.
- Public package file lists exclude generated `.metactl/` state.
- If a private overlay drove the release, it records the exact public commit, version, and tag it verified.

## Dependencies

- `cargo metadata --locked --format-version 1` succeeds.
- `cargo tree -d` has no unexplained duplicate dependency risk.
- `cargo audit` is run when available.
- Dependency license summary is reviewed before release.

Current scan record: `docs/release-readiness.md`.

## Build And Test

- `cargo fmt --check`
- `cargo check -p metactl -p metactld`
- `cargo test -p metactl`
- `make verify`
- `make verify-v1-release-gate`
- `cargo package -p metactl --allow-dirty --list`
- `cargo package -p metactld --allow-dirty --list`
- `cargo publish -p metactl --dry-run --allow-dirty`
- `cargo publish -p metactld --dry-run --allow-dirty` after `metactl` exists in the crates.io index

## Release

- Release artifacts include SHA-256 checksums.
- Release artifacts have GitHub provenance attestations.
- Published release notes include supported adapter tiers.
- Any vulnerability found during release prep is handled through `SECURITY.md`.
