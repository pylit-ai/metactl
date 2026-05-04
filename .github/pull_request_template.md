## Summary

## Verification

- [ ] `cargo fmt --check`
- [ ] `cargo test -p metactl`
- [ ] `cargo check -p metactl -p metactld`
- [ ] `make metactl-validate-contracts`
- [ ] `bash scripts/check_public_boundary.sh`

## Boundary Check

- [ ] No generated local agent roots
- [ ] No machine-specific paths
- [ ] No local-only packs or fixtures
- [ ] Commits are signed off with `Signed-off-by:`
