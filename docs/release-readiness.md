# Release Readiness

Last local verification: 2026-05-16.

## Dependency And License Scan

- `cargo metadata --locked --format-version 1`: passed.
- `cargo tree -d`: passed with no duplicate dependency versions reported.
- `cargo audit`: passed.
- `cargo publish -p metactl --dry-run --locked --allow-dirty`: run for `0.1.7` before publication.
- `cargo publish -p metactld --dry-run --locked --allow-dirty`: run after `metactl = 0.1.7` is published to crates.io.
- `cargo search metactl --limit 5`: verify `metactl = "0.1.7"` after publication.
- `cargo search metactld --limit 5`: verify `metactld = "0.1.7"` after publication.

License summary from Cargo metadata:

| License expression | Package count |
| --- | ---: |
| `MIT OR Apache-2.0` | 56 |
| `MIT` | 13 |
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
cargo test -p metactl
python3 scripts/verify_version_consistency.py
make verify
make verify-v1-release-gate
cargo package -p metactl --allow-dirty --list
cargo package -p metactld --allow-dirty --list
cargo run -p metactld -- --version
```

Release artifacts should be created through `.github/workflows/release.yml`, which produces SHA-256 checksums and GitHub provenance attestations.
The release workflow packages GitHub binary archives. crates.io publishing is run in dependency order because `metactld` depends on the matching published `metactl` crate version.

Publish order for crates.io:

1. Publish `metactl`.
2. Wait for `metactl = 0.1.7` to appear in the crates.io index.
3. Run `cargo publish -p metactld --dry-run`.
4. Publish `metactld`.

## Public/Private Release Sync

The public package manifests, public Git tag, and GitHub release are the source of truth for release versioning. A private overlay may drive release prep, but it should record the exact public commit, version, and tag it verified instead of defining a separate package version. Public release automation must not depend on the private overlay.
