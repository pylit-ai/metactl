# Agent Surfaces

This public reference names the target-owned surfaces metactl can materialize.
It is not an upstream standards dossier; use upstream vendor documentation for full behavior.

## Claude Code

- Root instruction document: `CLAUDE.md`
- Skills: `.claude/skills/<pack_id>/<surface_slug>/SKILL.md`
- Optional local-only document: `CLAUDE.local.md`

## Codex CLI

- Root instruction document: `AGENTS.md`
- Skills: `.codex/skills/<pack_id>/<surface_slug>/SKILL.md`

## Cursor

- Shared root instruction document: `AGENTS.md`
- Rules: `.cursor/rules/*.mdc`
- Skills: `.cursor/skills/<pack_id>/<surface_slug>/SKILL.md`
- Tier-1 evidence: `crates/metactl/tests/cursor_tier1.rs` verifies the always-applied `.mdc` rule frontmatter (`description`, `alwaysApply`) and applied skill bundle paths. Cursor Rules docs were checked at `https://cursor.com/docs/rules` on 2026-07-03.

## Filesystem Agent

- Root instruction document: `AGENTS.md`
- Generic pack resources: `.metactl/filesystem-agent/<pack_id>/<resource_name>`

## Gemini CLI

- Root instruction document: `GEMINI.md`
- Extension instructions: `.gemini/extensions/<pack_id>/GEMINI.md`
- Skills inside extensions: `.gemini/extensions/<pack_id>/skills/<surface_slug>/SKILL.md`
- Extension manifest: `.gemini/extensions/<pack_id>/gemini-extension.json`
- Commands inside extensions: `.gemini/extensions/<pack_id>/commands/<resource_name>`
- Tier-1 evidence: `crates/metactl/tests/gemini_tier1.rs` verifies manifest, context, command, and Agent Skill paths. Gemini CLI extension reference and Agent Skills docs were checked at `https://geminicli.com/docs/extensions/reference/` and `https://geminicli.com/docs/cli/skills/` on 2026-07-03.

## OpenClaw

- Root instruction document: `OPENCLAW.md`
