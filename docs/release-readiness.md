# Release Readiness

Release preparation: 2026-09-16. The release unit is the GitHub binary archives
and the `metactl` / `metactld` crates. The npm installer remains an unpublished
scaffold; publishing a new npm distribution is not part of this release.

## 0.1.21 Verification

The release includes the lock diagnostic repair and PRs #29, #28, and #30,
integrated in that order. The combined production code matches the independently
reviewed validation tree. Tests cover permission denial and recovery, project
root aliases, path containment, genuine lock contention, real fleet previews,
missing inputs, and partial fleet failures. Run the gates below on the final
release candidate; command outcomes and published commit identity belong in
the release notes.

### Release-candidate evidence

Fresh checks on the release candidate:

- `cargo test --workspace --locked`: 372 tests passed.
- `cargo audit`: passed with no advisories after updating the patched `anyhow` dependency.
- `make verify-v1-release-gate`: passed, including Docker installation from the packaged crate and the full surface benchmark (recall at three results: 1.0; false negatives: 0).
- Public boundary, documentation links and commands, version consistency, architecture budgets, adversarial MCP checks, and contract validation: passed.
- `make smoke-stdio smoke-cli smoke-dogfood`: passed, including installation and real command workflows.
- `cargo package -p metactl --list`: passed.
- `cargo package -p metactld --list`: passed.
- `cargo publish -p metactl --dry-run --locked`: passed.
- `cargo run -p metactl -- --version`: reports `metactl 0.1.21`.
- `cargo run -p metactld -- --version`: reports `metactld 0.1.21`.
- `cargo search metactl --limit 5`: still reports the previous published versions before publication.
- `cargo publish -p metactld --dry-run --locked`: intentionally deferred until `metactl = "0.1.21"` is visible in the crates.io index.

## Dependency And License Scan

- `cargo metadata --locked --format-version 1`: passed.
- `cargo tree -d`: passed with no duplicate dependency versions reported.
- `cargo audit`: passed.
- `cargo publish -p metactl --dry-run --locked`: run for `0.1.21` before publication.
- `cargo publish -p metactld --dry-run --locked`: run after `metactl = 0.1.21` is published to crates.io.
- `cargo search metactl --limit 5`: verify `metactl = "0.1.21"` after publication.
- `cargo search metactld --limit 5`: verify `metactld = "0.1.21"` after publication.

License summary from Cargo metadata:

| License expression | Package count |
| --- | ---: |
| `MIT OR Apache-2.0` | 56 |
| `MIT` | 15 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 13 |
| `Apache-2.0 OR MIT` | 5 |
| `Apache-2.0` | 2 |
| `MIT/Apache-2.0` | 2 |
| `Unlicense OR MIT` | 2 |
| `Apache-2.0 OR BSL-1.0` | 1 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 1 |
| `Zlib` | 1 |

Unknown license fields: 0.

## Release Gate Commands

```bash
bash scripts/check_public_boundary.sh
cargo fmt --check
cargo check -p metactl -p metactld
cargo test --workspace --locked
python3 scripts/verify_version_consistency.py
make verify
make verify-v1-release-gate
cargo package -p metactl --allow-dirty --list
cargo package -p metactld --allow-dirty --list
cargo run -p metactld -- --version
```

Release artifacts should be created through `.github/workflows/release.yml`, which produces SHA-256 checksums and GitHub provenance attestations.
Before publishing the draft, verify each archive with `gh attestation verify <archive> --repo pylit-ai/metactl`. Consumer verification behavior and the npm manual procedure are documented in [install verification](user/install-verification.md).
The release workflow packages GitHub binary archives. crates.io publishing is run in dependency order because `metactld` depends on the matching published `metactl` crate version.

Release notes for `0.1.21` are in [CHANGELOG.md](../CHANGELOG.md).

Publish order for crates.io:

1. Publish `metactl`.
2. Wait for `metactl = 0.1.21` to appear in the crates.io index.
3. Run `cargo publish -p metactld --dry-run`.
4. Publish `metactld`.

## Public/Private Release Sync

The public package manifests, public Git tag, and GitHub release are the source of truth for release versioning. A private overlay may drive release prep, but it should record the exact public commit, version, and tag it verified instead of defining a separate package version. Public release automation must not depend on the private overlay.
