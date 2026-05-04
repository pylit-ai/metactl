---
name: metactl-project-onboarding
description: Use when installing or configuring metactl in a greenfield or brownfield repository, selecting profiles, choosing packs or targets, binding a profile, syncing projected agent artifacts, installing the optional local MCP server, or verifying setup.
---

# metactl Project Onboarding

Use this skill as the primary greenfield or brownfield entrypoint for installing or repairing metactl in a repository without losing existing agent configuration. MCP setup is an optional branch of this workflow, not a separate starting point.

## Workflow

1. Inspect state before changing files.
   Run `metactl profile list`, `metactl status`, `metactl doctor`, and inspect existing target surfaces such as `AGENTS.md`, `CLAUDE.md`, `.claude/`, `.cursor/`, and `.gemini/`.
2. Classify the repo.
   Treat a repo with existing agent files as brownfield. Treat a repo with no metactl or agent surfaces as greenfield. If unsure, use brownfield behavior.
3. Choose the management layer.
   Use a user profile for reusable library roots and default packs. Use `metactl.yaml` for shared repo targets or intentional shared packs. Use `metactl.local.yaml` for personal-only additions.
4. Recommend a profile.
   Prefer an existing profile when its library roots and pack set match the repo. Create a new profile only when the workspace has a durable reusable shape that does not fit the existing profiles.
5. Pick targets from actual use.
   Use `metactl init --detect` for brownfield discovery, or choose explicit targets such as `--target codex-cli --target claude-code --target cursor --target gemini-cli` when the repo needs all supported agent surfaces.
6. Decide whether to bind the profile.
   Use plain `metactl init` when the profile should remain machine-local. Use `metactl init --bind-profile` or `metactl init --profile <name>` when the repo should record `extends_profile: <name>`.
7. Preview before applying in brownfield repos.
   Run `metactl sync --adopt preview` and inspect the proposed file changes. Do not use takeover unless the user explicitly asks to replace existing surfaces.
8. Apply with patch mode when safe.
   Run `metactl sync --adopt patch --yes` only after preview is acceptable. Runtime settings such as `.claude/settings.json` should merge rather than erase unrelated user keys.
9. Install refresh hooks when desired.
   Run `metactl hook install` so checkout and merge operations refresh metactl-managed surfaces after config changes.
10. Decide whether to install the local MCP server.
   Install MCP when the user wants coding agents to query metactl through native tools for pack search, explanations, compile previews, or validation. Skip MCP for one-off CLI-only setup or when the user does not want agent MCP config files changed.
11. Install MCP with the documented helper when selected.
   Use `docs/mcp/servers.md` as the reference when the metactl docs are available, and run the one-line install for the user's client, such as `make metactl-mcp-install MCP_CLIENT=cursor`. When installing into another repository from a metactl checkout, set `MCP_PROJECT_ROOT=/path/to/repo`. In brownfield repos, inspect existing MCP config first and preserve unrelated server entries.
12. Verify and report.
   Run `metactl doctor`, `metactl --json status`, and `metactl explain --json`. If MCP was installed, run the MCP smoke check from `docs/mcp/servers.md` or the client's MCP list command. Report the active profile, packs, targets, changed files, MCP config path, and exact commands used.

## Command Patterns

- List profiles: `metactl profile list`
- Show effective state: `metactl --json status`
- Inspect why packs and surfaces were selected: `metactl explain --json`
- Greenfield default-profile setup: `metactl init --target codex-cli`
- Brownfield detection: `metactl init --detect`
- Shared profile binding: `metactl init --profile <name>` or `metactl init --bind-profile`
- Personal pack addition: `metactl use --local <pack>`
- Brownfield preview: `metactl sync --adopt preview`
- Brownfield patch apply: `metactl sync --adopt patch --yes`
- Refresh hooks: `metactl hook install`
- MCP install reference: `docs/mcp/servers.md`
- Claude Code MCP install: `make metactl-mcp-install MCP_CLIENT=claude-code`
- Cursor MCP install: `make metactl-mcp-install MCP_CLIENT=cursor`
- Gemini CLI MCP install: `make metactl-mcp-install MCP_CLIENT=gemini-cli`
- Codex CLI MCP install: `make metactl-mcp-install MCP_CLIENT=codex-cli MCP_SCOPE=user`
- MCP install into another repo: `make -C /path/to/metactl metactl-mcp-install MCP_CLIENT=cursor MCP_PROJECT_ROOT="$PWD"`
- MCP smoke test: `make -C /path/to/metactl metactl-mcp-smoke`
- MCP direct smoke check: `metactld --mcp --once <(printf '%s\n' '{"jsonrpc":"2.0","id":"tools","method":"tools/list","params":{}}') --library-root "$PWD/library/starter"`

## Guardrails

- Never delete, replace, or normalize existing agent settings just to make metactl output simpler.
- Treat `.claude/settings.json`, `.codex/config.toml`, and similar runtime files as shared surfaces unless metactl clearly owns them already.
- Prefer patch or preview for brownfield repos. Reserve takeover for explicit destructive approval.
- Do not add every available pack to a default profile. Keep default context small and put specialized packs in specialist profiles or local overrides.
- Do not copy private user-library artifacts into public starter packs.
- If profile roots do not include roles, policies, or targets, add the starter library root to the profile or project config before syncing.
- Do not configure MCP until `metactld` is installed on `PATH` or the install command has a reviewed absolute `--metactld-bin` path.
- Do not overwrite unmanaged MCP server entries. Use `--force` only after reviewing the existing `metactl` entry with the user.
- Treat the MCP server as local and read-only. Do not present it as a hosted service, remote control plane, or mutating automation surface.

## Output Format

- State summary: `profile`, `profile source`, `library roots`, `targets`, `packs`, `brownfield findings`
- Recommendation: profile choice, targets, pack set, and whether to bind the profile
- Apply plan: preview command, apply command, hook command, optional MCP install command, verification commands
- Result: changed files, projected surfaces, MCP config path and verification result if installed, remaining warnings, and exact commands used
