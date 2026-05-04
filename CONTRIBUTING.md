# Contributing

## Local Checks

- `cargo fmt --check`
- `cargo test -p metactl`
- `cargo check -p metactl -p metactld`
- `make metactl-validate-contracts`
- `bash scripts/check_public_boundary.sh`

Keep public changes focused on the local CLI/kernel, stable contracts, generic fixtures, and public docs.
Do not add local-only packs, local agent configs, internal planning specs, generated adapter trees, or local machine paths.

## Developer Certificate of Origin

All commits must include a `Signed-off-by:` trailer certifying the Developer Certificate of Origin 1.1.

Use:

```bash
git commit -s
```

By contributing, you certify that you have the right to submit the work under this repository's license.
