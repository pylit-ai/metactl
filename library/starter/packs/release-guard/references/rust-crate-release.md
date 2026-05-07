# Rust/Cargo Release Protocol

Use this procedure for Rust workspaces, CLIs, and libraries that publish GitHub tag releases or crates.io packages.

## Inputs

- Public repo and expected remote.
- Package names and workspace members.
- Version source, usually each crate's `Cargo.toml`.
- Release tag, usually `vX.Y.Z`.
- Release workflow trigger, usually tag push or manual `workflow_dispatch`.
- Publish target: GitHub release, crates.io, both, or internal handoff only.

## Preflight

Run before staging:

```bash
git status --porcelain=v1
git rev-parse HEAD
git remote -v
git tag --sort=-v:refname
git ls-remote --tags origin
cargo metadata --locked --format-version 1
cargo tree -d
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
```

Add repo-specific gates, for example:

```bash
bash scripts/check_public_boundary.sh
make verify
```

For private-overlay releases, run the private leak check before touching the public release tag.

## Version And Tag

1. Read the version from the package manifest.
2. Check local and remote tags for `vX.Y.Z`.
3. Check the registry if publishing to crates.io.
4. If a tag or registry version already exists, bump to the next valid version before commit.
5. Keep internal workspace dependency versions aligned.

For a workspace where `metactld` depends on `metactl = "X.Y.Z"`, publish order is:

1. `cargo publish -p metactl --dry-run --locked`
2. `cargo publish -p metactl --locked`
3. Wait for `metactl = X.Y.Z` in the crates.io index.
4. `cargo publish -p metactld --dry-run --locked`
5. `cargo publish -p metactld --locked`

## Package Gates

Run before pushing a tag:

```bash
cargo package --workspace --locked --list
cargo publish --dry-run --workspace --locked
```

If a workspace member cannot dry-run because an unpublished sibling crate is not in the registry yet, dry-run and publish in dependency order instead of weakening the gate.
For binary archive workflows, it is acceptable to skip packaging a dependent crate until its sibling dependency exists in the registry, but still build and smoke-test the dependent binary.

## Commit, Push, Release

1. Stage only intended release files.
2. Commit with a message that names the release unit or feature.
3. Push the commit branch.
4. Wait for required CI on the pushed commit.
5. Create an annotated tag: `git tag -a vX.Y.Z -m "Release vX.Y.Z"`.
6. Push the tag.
7. Watch the release workflow until it is green.
8. Verify artifacts, checksums, provenance, and install smoke tests.

## Recovery

- Bad draft GitHub release: delete or edit the draft before publishing.
- Bad tag before public consumption: delete local and remote tag, then retag the fixed commit.
- Bad crates.io package: yank the version; crates.io does not allow deleting or replacing a published version.
- Bad binary archive: publish a corrective patch release and mark the broken artifact clearly.
