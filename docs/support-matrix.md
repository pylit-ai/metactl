# Support Matrix

Support tiers describe what this repository verifies today.

| Target | Tier | Evidence |
| --- | --- | --- |
| Codex CLI | Tier 1, conformance-covered | Public fixtures and smoke tests cover generated Codex surfaces. |
| Claude Code | Tier 1, conformance-covered | Public fixtures and smoke tests cover generated Claude surfaces. |
| Cursor | Tier 1, conformance-covered | `cargo test -p metactl cursor_tier1_generates_project_rule_index_and_skill_bundle` verifies `.cursor/rules/*.mdc` frontmatter and `.cursor/skills/.../SKILL.md` surfaces against Cursor Rules docs checked 2026-07-03. |
| Filesystem Agent | Experimental | Generic descriptor fixture for agents that read files from a project tree. |
| Gemini CLI | Tier 1, conformance-covered | `cargo test -p metactl gemini_tier1_generates_extension_manifest_context_commands_and_skills` verifies `GEMINI.md`, `.gemini/extensions/.../gemini-extension.json`, commands, and Agent Skill folders against Gemini CLI extension/skills docs checked 2026-07-03. |

## Tier Definitions

- Tier 1: release-blocking fixtures and smoke tests exist.
- Tier 2: fixtures exist, but failures may not block a release.
- Experimental: examples may exist, but compatibility is not claimed.

Compatibility statements are valid only for the released `metactl` version and the target versions tested in CI. External format checks were last refreshed against public vendor docs on 2026-07-03.
