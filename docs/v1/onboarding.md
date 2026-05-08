# metactl v1 Onboarding

This path is the public v1 onboarding skeleton. It stays generic: no private library paths, account names, customer details, internal URLs, or local machine state.

## Prerequisites

- Read [charter.md](charter.md) before adding sources, targets, or merge behavior.
- Read [decisions/private-by-default-sanitized-export.md](decisions/private-by-default-sanitized-export.md) before sharing examples or generated projections.
- Review [../user/GETTING_STARTED.md](../user/GETTING_STARTED.md) for the current CLI setup path.

## Local First Run

For the default local-only flow:

```bash
metactl library init --user --profile user
metactl project link --profile user
metactl sync --target codex,claude,cursor,gemini --preview
metactl sync --target codex,claude,cursor,gemini --apply
metactl check --strict
```

The profile hides the simple v1 model from the first-run path: 0..N pinned read-only baselines plus one writable user overlay. Preview generated project projections before writing target-native files. Validation fails if staged or applied outputs drift from the lock.

## Verification

Use these gates while the v1 control plane is under active development:

```bash
make verify-v1-charter
make verify-public-boundary
make verify-docs-links
```

Expected result: all three commands exit 0 and print an OK signal.

## Non-Goals

Do not use onboarding to introduce hosted registries, agent runtimes, public-by-default source trees, or private customer examples. Those proposals must be rejected or moved to a future design record outside v1.
