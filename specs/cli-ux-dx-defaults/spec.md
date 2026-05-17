# CLI UX/DX Defaults Spec

## Status

Active public spec for `cli-ux-dx-defaults`.

## Scope

This change makes high-frequency `metactl` workflows safer and easier to discover without changing the core product model. Public defaults remain agent-agnostic: no runtime is treated as the implicit public default unless a project config, machine profile, environment variable, explicit flag, or wizard choice selects it.

## Requirements

- Bare object-group commands must resolve to safe read-only defaults: `target` to `target list`, `source` to `source list`, `profile` to `profile show`, `ignore` to `ignore status`, `audit` to `audit sources`, `fleet` to `fleet status`, and `demo` to `demo list`.
- `preview` must be an additive alias for `sync --preview`; `sync` compatibility remains intact.
- Public `init` must refuse to guess when no target is specified, no profile target exists, and no target surface is detected. Non-interactive and agent mode must return exact next commands.
- Global `--agent` must imply JSON, no prompts, no human-only output, and stable recoverable-error fields.
- `source sync` without a name must sync all configured sources, while missing-source errors must list configured sources and next commands.
- `source add <LOCATION>` must infer a source id from public manifest metadata when present, or from the basename when unambiguous. `source add <NAME> <LOCATION>` remains compatible.
- `pack use`, `pack add`, and `pack remove` must be aliases for project pack activation, while Agent Skill import/export commands stay distinct.
- Built-in public profile templates must be visible and must not depend on private library paths: `neutral`, `multi-agent`, `agent-ci`, `solo-codex`, and `private-overlay`.

## Evidence Patterns

- CLI Guidelines: missing arguments should produce concise help, safe defaults, machine-readable output, and explicit exit status.
- GitHub CLI: current-directory object commands can default to current object views while JSON remains opt-in and explicit.
- uv: high-frequency project verbs can be top-level aliases when they preserve compatibility and remain documented as conveniences.
- OpenAI shell-tool guidance: agent execution needs audited commands, stable exit status, and sandbox-safe non-interactive behavior.
- Anthropic Claude Code guidance: agent-facing CLIs should expose predictable verification and context-management flows.

## Top-Level Command Exception Criteria

Top-level aliases are accepted only when they are high-frequency, reversible or preview-first, already represented by an explicit object command, and documented as convenience aliases rather than replacements. Unknown root arguments remain errors.

