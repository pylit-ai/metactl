# CLI UX/DX Defaults Plan

## Steps

1. Add focused CLI workflow tests for bare command defaults, `preview`, `--agent`, source recovery, pack aliases, and profile templates.
2. Update CLI parsing so selected command groups accept an omitted subcommand and dispatch to the documented read-only default.
3. Add `preview` and `pack use/add/remove` aliases without removing existing commands.
4. Add `--agent` as a global machine contract with enriched recoverable-error JSON.
5. Extend source add/sync recovery defaults while preserving explicit forms.
6. Expose public built-in profile templates and document how profile files live under the user config directory.
7. Update user-facing docs and run the verifier ladder.

## Compatibility

Existing explicit commands remain valid. `--json` remains compatible; `--agent` is additive and implies `--json --no-input`.

