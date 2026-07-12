# Changelog

## 0.1.21 - 2026-07-03

### Added

- Added a working `metactl help --all` command that lists hidden advanced commands.
- Added `verify` as an alias for `validate`.
- Added a total agent error envelope with stable `project_not_found` and `next_commands` fields.
- Added bounded-by-default agent output with `--full` for complete lists.
- Added `profile_resolution` disclosure and `--no-profile` to control profile inheritance.
- Added Cursor Tier-1 conformance coverage for generated `.cursor/rules/*.mdc` and `.cursor/skills/.../SKILL.md` surfaces.
- Added Gemini CLI Tier-1 conformance coverage for `GEMINI.md`, `.gemini/extensions/.../gemini-extension.json`, command files, and Agent Skill folders.
- Added an Experimental OpenCode target that generates `AGENTS.md`, `opencode.json`, `.opencode/commands/...`, `.opencode/skills/.../SKILL.md`, and `.opencode/packs/...` surfaces.
- Added `scripts/verify_opencode_target.sh` to prove OpenCode target add, compile, and validation behavior without Rust source changes.
- Added `status` instruction-noise reporting for drifted managed outputs, stray unmanaged agent surfaces, and duplicate trigger metadata.
- Added a dirty-worktree warning that lets sync proceed while disclosing local changes.
- Added `explain --capabilities` and the repository `llms.txt` capability-discovery reference.
- Added import-stub bridge mode for managed Claude Code imports of `AGENTS.md`.
- Added Ruler and AgentSync project importers.
- Added fleet machine-contract parity for recoverable agent-mode failures.
- Added the `metactl check` GitHub Action for verified CI drift checks.
- Added binstall, Homebrew, and npm installation paths.

### Changed

- Reduced the default top-level CLI help to the daily porcelain commands while preserving existing advanced commands for scripts and expert workflows.
- Rewrote the README onboarding path around the individual developer workflow: define agent instructions once, review generated files, and work across supported coding agents.
- Promoted Cursor and Gemini CLI support-matrix entries from Tier 2 preview to Tier 1 conformance-covered.
- Added OpenCode to the support matrix as Experimental.
- Added OpenCode compatibility metadata for starter packs used by the default and test workflows.

### Documented

- Added a private spec slice for per-pack managed blocks in brownfield documents.
- Documented the CLI porcelain consolidation proposal and measured warm-checkout time-to-value evidence.

### Not Included

- No runtime implementation of per-pack managed blocks.
- No proprietary-runtime end-to-end execution inside Cursor, Gemini CLI, or OpenCode.
- No publishing, Git tag, or GitHub release action.
