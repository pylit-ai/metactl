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

## Filesystem Agent

- Root instruction document: `AGENTS.md`
- Generic pack resources: `.metactl/filesystem-agent/<pack_id>/<resource_name>`

## Gemini CLI

- Root instruction document: `GEMINI.md`
- Extension instructions: `.gemini/extensions/<pack_id>/GEMINI.md`
- Skills inside extensions: `.gemini/extensions/<pack_id>/skills/<surface_slug>/SKILL.md`

## OpenClaw

- Root instruction document: `OPENCLAW.md`
