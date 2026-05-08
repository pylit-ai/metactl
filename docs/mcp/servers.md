# metactl MCP Servers

## Purpose

This is the canonical inventory for metactl-provided MCP servers, their trust boundaries, and installation commands.

For greenfield or brownfield repository setup, start with the `metactl-project-onboarding` skill. That skill owns the install workflow, repo classification, profile choice, sync/adoption mode, hook setup, optional MCP install, and verification. This document is the reference for the MCP branch of that workflow.

As of 2026-04-23, the recommended shape is to keep MCP setup as an optional onboarding step instead of a separate skill. A dedicated MCP setup skill would be justified only if the MCP surface grows an independent lifecycle such as remote HTTP serving, auth/OAuth setup, secret-bearing configuration, multi-server fleet management, or client-specific debugging runbooks.

## Server Inventory

| Server | Binary | Transport | Scope | Tools | Trust boundary |
|---|---|---|---|---|---|
| `metactl` | `metactld` | stdio | local process | `metactl_search_packs`, `metactl_explain`, `metactl_compile_preview`, `metactl_validate` | Read-only adapter over the local kernel. Negotiates MCP protocol `2025-11-25` or `2025-06-18` from the client initialize request. No `apply`, `revert`, install, network, or secret-bearing tools. Compile preview stages into an ephemeral scratch directory and ignores caller `project_root`. |

## One-Line Install

Run these from the metactl repository root. They install `metactld` onto `PATH` with Cargo, then add a `metactl` MCP server entry for the selected agent.

By default the project-scoped commands write config into the current metactl checkout. To install into another greenfield or brownfield repository, add `MCP_PROJECT_ROOT=/path/to/repo`, or run from that repository with `make -C /path/to/metactl ... MCP_PROJECT_ROOT="$PWD"`.

MCP install is optional. Use it when agents should discover packs, inspect explanations, preview compiled surfaces, or validate config through native tool calls. Skip it for CLI-only setup or when the user does not want agent MCP config changed.

| Agent | Scope | Command |
|---|---|---|
| Claude Code | project `.mcp.json` | `make metactl-mcp-install MCP_CLIENT=claude-code` |
| Cursor | project `.cursor/mcp.json` | `make metactl-mcp-install MCP_CLIENT=cursor` |
| Gemini CLI | project `.gemini/settings.json` | `make metactl-mcp-install MCP_CLIENT=gemini-cli` |
| Codex CLI | user `~/.codex/config.toml` | `make metactl-mcp-install MCP_CLIENT=codex-cli MCP_SCOPE=user` |

Example from a repository being onboarded, using a separate metactl checkout:

```bash
make -C /path/to/metactl metactl-mcp-install MCP_CLIENT=cursor MCP_PROJECT_ROOT="$PWD"
```

**Expected output:**

```text
Installed metactl MCP server for cursor.
Config: /path/to/repo/.cursor/mcp.json
Server: metactl
```

The helper is idempotent for entries it owns. It refuses to overwrite an existing unmanaged `metactl` server unless run with `--force`:

```bash
python3 scripts/install_metactl_mcp.py cursor --force
```

**Expected output:**

```text
Updated .cursor/mcp.json
Server `metactl` now points at metactld --mcp --stdio.
```

## Direct Server Command

All client entries point at the same stdio server shape:

```bash
metactld --mcp --stdio --library-root "$PWD/library/starter"
```

This command starts a stdio server and waits for MCP input. It normally does not print a success banner.

Use an absolute `--library-root` in committed or user-level config because MCP clients may spawn the server from a different working directory.

## Client Notes

- **Claude Code:** project-scoped MCP servers are stored in root `.mcp.json`; local and user scopes are managed by `claude mcp add`.
- **Cursor:** Cursor uses `mcpServers` JSON in `.cursor/mcp.json` for project scope or `~/.cursor/mcp.json` for user scope.
- **Gemini CLI:** Gemini uses `mcpServers` in `.gemini/settings.json` or `~/.gemini/settings.json`. The installer uses the server name `metactl` without underscores.
- **Codex CLI:** Codex custom MCP servers are documented under user `~/.codex/config.toml`; the installer writes a marked managed block and sets `default_tools_approval_mode = "prompt"`.

## Native CLI Equivalents

Prefer the installer above for consistency. These native commands are useful when debugging a specific client.

```bash
claude mcp add --transport stdio --scope project metactl -- "$(command -v metactld)" --mcp --stdio --library-root "$PWD/library/starter"
```

**Expected output:**

```text
Added project-scoped MCP server `metactl`.
```

```bash
gemini mcp add -s project metactl "$(command -v metactld)" -- --mcp --stdio --library-root "$PWD/library/starter"
```

**Expected output:**

```text
Added MCP server `metactl`.
```

## Verification

After install, prefer the smoke target because it uses the configured default starter library and fails with the underlying MCP error instead of printing `null` through a narrow JSON selector:

```bash
make metactl-mcp-smoke
```

From another repository, run it through the metactl checkout:

```bash
make -C /path/to/metactl metactl-mcp-smoke
```

**Expected output:**

```text
ok negotiated protocol: 2025-06-18
ok tools: metactl_search_packs, metactl_explain, metactl_compile_preview, metactl_validate
ok search first match: metactl-project-onboarding
```

If you run the server by hand, use an absolute library root:

```bash
metactld --mcp --once <(printf '%s\n' '{"jsonrpc":"2.0","id":"tools","method":"tools/list","params":{}}') --library-root "/path/to/metactl/library/starter"
```

**Expected JSON shape:**

```json
{
  "jsonrpc": "2.0",
  "id": "tools",
  "result": {
    "tools": [ ... ]
  }
}
```

Client-specific checks:

| Agent | Check |
|---|---|
| Claude Code | `claude mcp list` or `/mcp` inside Claude Code |
| Cursor | `cursor-agent mcp list`; run `cursor-agent mcp enable metactl` once if it says the server needs approval, then `cursor-agent mcp list-tools metactl` |
| Gemini CLI | `gemini mcp list` or `/mcp` inside Gemini CLI |
| Codex CLI | Start Codex and use `/mcp` |

## Source Review

Install surfaces were checked on 2026-04-23 against:

- Claude Code MCP docs: https://code.claude.com/docs/en/mcp
- Cursor MCP config docs: https://docs.cursor.com/context/mcp
- Cursor CLI MCP docs: https://docs.cursor.com/cli/mcp
- Codex config docs: https://github.com/openai/codex/blob/main/docs/config.md
- Gemini CLI MCP docs: https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md
