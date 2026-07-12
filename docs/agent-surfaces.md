# Agent Surfaces

This public reference names the target-owned surfaces metactl can materialize.
It is not an upstream standards dossier; use upstream vendor documentation for full behavior.

## Agent error contract

`metactl --agent` makes every command failure machine-readable, including command-line usage errors that occur before a command handler runs. Each error is one JSON object on standard output with `ok: false`, `api_version`, `error_code`, `message`, and a non-empty `next_commands` array.

- Usage and argument parsing failures use `error_code: "usage"` and exit code 10.
- An explicit `--project` path that is missing or is not a directory uses `error_code: "project_not_found"` and exit code 10.
- Validation and check failures include recovery commands; drifted output suggests `metactl sync --adopt preview` before `metactl sync --adopt patch`.

Human-mode parse errors retain Clap's normal formatted help and diagnostics.

## Bounded machine output

Machine output (`--agent` or `--json`) bounds long lists by default so a command response remains safe to place in agent context. Lists longer than 15 entries retain the first 15 values and add sibling fields named `<list>_truncated: true` and `<list>_total_count: N`; the original list field remains present so consumers can detect the shortened response without a schema-version change.

Consumers MUST check `<list>_truncated` before treating any array as complete; arrays without the sibling marker are complete.

Pass the global `--full` flag with `--agent` or `--json` to restore complete list enumeration. Human output is unchanged.

## Capability discovery

Run `metactl explain --capabilities --json` from any directory to obtain the stable machine manifest: command surface, supported global flags, exit-code labels, error-envelope contract, truncation markers, and target capability matrices. Human mode prints a compact listing. The command does not require a project root.

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

## OpenCode

- Root instruction document: `AGENTS.md`
- Project config: `opencode.json`
- Commands: `.opencode/commands/<resource_name>`
- Skills: `.opencode/skills/<surface_slug>/SKILL.md`
- Pack resources referenced from config: `.opencode/packs/<pack_id>/<resource_name>`
- Experimental evidence: `scripts/verify_opencode_target.sh` verifies generated OpenCode paths. OpenCode config, rules, commands, and Agent Skills docs were checked at `https://opencode.ai/docs/config/`, `https://opencode.ai/docs/rules/`, `https://opencode.ai/docs/commands/`, and `https://opencode.ai/docs/skills/` on 2026-07-03.
