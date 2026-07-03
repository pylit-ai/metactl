# Changelog

## 0.1.21 - 2026-07-03

### Added

- Added Cursor Tier-1 conformance coverage for generated `.cursor/rules/*.mdc` and `.cursor/skills/.../SKILL.md` surfaces.
- Added Gemini CLI Tier-1 conformance coverage for `GEMINI.md`, `.gemini/extensions/.../gemini-extension.json`, command files, and Agent Skill folders.
- Added an Experimental OpenCode target that generates `AGENTS.md`, `opencode.json`, `.opencode/commands/...`, `.opencode/skills/.../SKILL.md`, and `.opencode/packs/...` surfaces.
- Added `scripts/verify_opencode_target.sh` to prove OpenCode target add, compile, and validation behavior without Rust source changes.
- Added `status` instruction-noise reporting for drifted managed outputs, stray unmanaged agent surfaces, and duplicate trigger metadata.

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
