# Release Readiness

Release candidate: **0.1.23**, prepared September 25, 2026. Distribution unit:
GitHub native binary archives and existing metactl/metactld crates, in dependency
order. npm remains an unpublished scaffold; no new registry distribution.

This release adds bounded gateway-backed discovery trials, routing receipts,
private event inspection and first-run activation guidance. It also includes
Codex projection safety and library source-drift planning since v0.1.22. Native
defaults remain unchanged. See [activation](user/skill-discovery.md),
[trial modes](user/discovery-trials.md), and the [changelog](../CHANGELOG.md).

## Evidence and claims

The v0.1.22 discovery implementation passed 373 Rust and 19 Python tests.
This candidate adds gateway trials and onboarding guidance. Run all final
gates on the release commit; completed test counts, CI,
checksums, attestations and publication identity belong in its release notes.
No provider quality, native prompt-token or session-cost improvement is claimed.

## Required release gates

```bash
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo audit
make verify
cargo package -p metactl --locked
cargo publish -p metactl --dry-run --locked
```

Review Cargo metadata/license fields and dependency changes. Validate the embedded
host mirror through the discovery suite; test status, fail-closed checks and MCP
stdio from the packaged binary without a source checkout. The optional host needs
Python 3.10+ at runtime; other CLI commands do not.

Release automation in `.github/workflows/release.yml` creates a draft after both
supported platform packages pass. Download both archives, verify SHA-256 sidecars
and `gh attestation verify` provenance, then run clean installation smoke checks
before publishing. See [install verification](user/install-verification.md).

## Publication order and recovery

1. Push candidate and require green exact-head CI; merge through normal PR flow.
2. Verify merged main, then push an annotated v0.1.23 tag. Never move a published tag.
3. Verify workflow and draft assets before public release.
4. Publish metactl to crates.io after its dry-run passes.
5. Wait for metactl 0.1.23 to be visible; dry-run and publish metactld next.
6. Verify both registry versions and installed binary provenance.

A private overlay records the same public version and tag, never a competing
release identity. Retain installed-binary backups through rollout verification.
If artifacts fail, keep the draft unpublished and fix forward; do not silently
retag. Feature rollback removes only the optional MCP registration or selects
deterministic ranking, leaving native instructions intact.
