# Guided Human CLI UX And Agent Contracts

This spec extends `cli-ux-dx-defaults`. It does not replace the existing command
model, sync workflow, or v2 JSON contract.

## Setup

`metactl setup` is a top-level guided entrypoint for first-run project setup. It
is allowed as a top-level exception because first-run users need a memorable
command before they know metactl's `init`, `ignore`, and `sync` subcommands.

Required behavior:

- `metactl setup --plan` prints the files, choices, and equivalent raw commands
  without writing project state.
- `metactl --agent setup --plan` emits JSON and never prompts.
- Non-interactive setup must either have enough explicit flags to proceed or
  return recoverable JSON with `next_commands`.
- `metactl setup --target <id> --yes` may create `metactl.yaml` and lock state,
  but must not run `sync` or materialize runtime adapter files.
- Existing `metactl.yaml` is preserved unless an explicit future overwrite flag
  is introduced.

## Ignore Repair

`metactl ignore status` diagnoses local and repo ignore posture, target
resolution, tracked generated roots, fix availability, and next commands.

`metactl ignore fix` plans or applies a repair:

- `--plan` reports intended writes and untrack actions without writing files or
  changing the Git index.
- `--scope local|repo|both` controls `.git/info/exclude`, `.gitignore`, and
  target allowlist files.
- `--include-lock` and `--include-private-sources` are explicit opt-ins.
- Generated-root untracking requires `--untrack-generated --yes` in
  non-interactive mode.
- Untracking uses Git index removal only. Files must remain on disk.

Root adapter docs such as `AGENTS.md`, `CLAUDE.md`, and `GEMINI.md` are not
classified as generated roots by this repair contract because teams may
intentionally version them.

## Doctor And Agent Safety

`metactl doctor` remains diagnostic-only. It may report setup posture, ignore
repair checks, recoverable next commands, and a fix-plan reference. It must not
write ignore files, initialize config, run sync, or mutate the Git index.

All `--agent` and `--no-input` paths must be non-interactive. JSON additions are
additive and must preserve the existing success and error envelope fields.

## Acceptance Criteria

- Agent setup and ignore repair planning return parseable JSON.
- `ignore fix --plan` reports write and untrack actions without mutation.
- Authorized generated-root untracking removes files from the Git index without
  deleting them from disk.
- `--yes` alone does not authorize generated-root untracking in non-interactive
  mode.
- Doctor points to `metactl ignore fix --plan` when tracked generated roots or
  missing ignore posture are detected.
