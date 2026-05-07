---
name: release-guard
description: Use before packaging, tagging, publishing, or handing off a release, especially Rust/Cargo crates, CLIs, GitHub tag releases, and private-overlay public releases.
tags: [ "release", "verification", "pack:core" ]
---

# Release Guard

Use this skill to convert vague "looks ready" claims into an explicit release gate.

## Workflow

1. Confirm the release unit.
   Identify the public repo, package names, binaries, version, tag, publish registry, and whether a private overlay is driving the public release.
2. Inspect state before changing it.
   Record branch, commit SHA, status, remotes, local tags, remote tags, and existing package versions. Do not infer release freshness from local files alone.
3. Choose the version and tag.
   Use the repo's declared version source (`Cargo.toml`, `pyproject.toml`, package manifest, or release config). If a tag already exists locally or remotely, stop and choose the next valid version before publishing.
4. Run release gates before commit.
   Execute the repo-specific local gates and any private-overlay leak/boundary checks. Treat skipped, unavailable, or failing gates as release blockers unless the user explicitly accepts the risk.
5. Commit only the intended release diff.
   Stage a reviewed set of files. Do not include unrelated local edits, generated caches, private notes, credentials, or environment files.
6. Push code before tags.
   Push the release commit first, observe required remote CI until green, then create and push the tag or trigger the release workflow.
7. Verify release artifacts.
   Confirm the GitHub release, registry package, checksums, provenance attestations, and install smoke tests appropriate to the project.
8. Record recovery.
   Note exactly how to yank/unpublish where supported, delete a draft release, delete a bad tag, or publish a corrective patch.

## Rust/Cargo Procedure

Use `references/rust-crate-release.md` for the detailed Rust path. Minimum gates for a Rust CLI/library repo:

```bash
cargo fmt --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo package --workspace --locked --list
cargo publish --dry-run --workspace --locked
```

Adjust for workspace dependency order. If one crate depends on another workspace crate by published version, dry-run and publish the dependency crate first, wait for the registry index, then dry-run and publish the dependent crate.

For repos with public/private split, also run the private leak check and public boundary check before commit and before tag push.

## Output Format

- Release unit:
- Version and tag:
- Local gates:
- Remote gates:
- Published artifacts:
- Remaining risk:
- Recovery path:

## Guardrails

- Do not treat unrun validation as implied green status.
- Do not push a tag before the release commit is on the intended branch.
- Do not call a release complete until required remote CI and release workflows are green.
- Do not publish a dependent Cargo crate until its dependency crate version exists in the registry index.
- Do not include private-overlay instructions, local paths, account names, credentials, or generated agent state in public release artifacts.
- Do not hand off a release without a recovery note.
- Use `references/checklist.md` for the portable checklist and `references/rust-crate-release.md` for Rust/Cargo releases.
