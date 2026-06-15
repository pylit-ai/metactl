# Release Readiness

Last local verification: 2026-05-04.

## Dependency And License Scan

- `cargo metadata --locked --format-version 1`: passed.
- `cargo tree -d`: passed with no duplicate dependency versions reported.
- `cargo audit`: passed.
- `cargo publish -p metactl --dry-run --allow-dirty`: passed.
- `cargo publish -p metactld --dry-run --allow-dirty`: deferred until `metactl` is published to crates.io, because `metactld` depends on `metactl = 0.1.18`.

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
make verify
cargo package -p metactl --allow-dirty --list
cargo package -p metactld --allow-dirty --list
```

Release artifacts should be created through `.github/workflows/release.yml`, which produces SHA-256 checksums and GitHub provenance attestations.

Publish order for crates.io:

1. Publish `metactl`.
2. Wait for `metactl = 0.1.18` to appear in the crates.io index.
3. Run `cargo publish -p metactld --dry-run`.
4. Publish `metactld`.
