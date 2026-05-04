# Conformance

`metactl-compatible` means a target or pack passes the public conformance checks for a specific release.

## Badge Rules

A project may claim `metactl-compatible` only when:

- it identifies the tested `metactl` version;
- public fixtures or equivalent tests pass;
- generated output is deterministic for the same inputs;
- generated files do not require machine-specific paths;
- failures are reported with stable machine-readable diagnostics.

## Local Checks

```bash
make verify
bash scripts/check_public_boundary.sh
```

## Claims

Use precise claims:

- "passes metactl 0.1.0 Codex CLI conformance"
- "tested with metactl 0.1.0"

Avoid broad claims such as "official" or "certified" unless maintainers have published a release-specific compatibility note.
