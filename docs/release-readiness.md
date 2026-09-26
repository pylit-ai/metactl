# Release Readiness

Release candidate: **0.1.25**, prepared September 26, 2026. Distribution unit:
GitHub native binary archives. Crate packaging and dry-run checks remain gates;
registry publication is a separate, explicitly verified action. npm remains an
unpublished scaffold; no new registry distribution is included.

This release repairs ignore migration and preserves protected private skills,
resources, hooks and runtime configuration. Native Git evaluates proposed rules
before private publication; current synthesis evidence distinguishes shared
outputs from private or unknown content. Recovery retains displaced files on
macOS, Linux and Windows. See [private projection protection](user/private-projection-protection.md)
and the [changelog](../CHANGELOG.md).

## Evidence and claims

Run all final gates on the release commit; completed test counts, CI,
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
before publishing. The workflow exercises fresh init/use/sync/validate and the
packaged discovery host from each assembled archive on its native runner, using
an isolated home and no provider credentials. See [install verification](user/install-verification.md).

## Publication order and recovery

1. Push candidate and require green exact-head CI; merge through normal PR flow.
2. Verify merged main, then push an annotated v0.1.25 tag. Never move a published tag.
3. Verify workflow and draft assets before public release.

If crate publication is separately included in the release scope, publish
metactl only after its dry-run passes, wait for metactl 0.1.25 to be visible,
then dry-run and publish metactld. Verify both registry versions independently;
the GitHub release does not imply registry publication.

A private overlay records the same public version and tag, never a competing
release identity. Retain installed-binary backups through rollout verification.
If artifacts fail, keep the draft unpublished and fix forward; do not silently
retag. Feature rollback removes only the optional MCP registration or selects
deterministic ranking, leaving native instructions intact.
