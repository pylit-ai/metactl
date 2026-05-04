#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LIBRARY_ROOT = ROOT / "library" / "starter"
SERVER_NAME = "metactl"
SERVER_ARGS = ["--mcp", "--stdio", "--library-root"]
BEGIN_MARKER = "# BEGIN metactl MCP server"
END_MARKER = "# END metactl MCP server"


def load_json(path: Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def resolve_metactld(explicit: str | None) -> str:
    if explicit:
        return str(Path(explicit).expanduser().resolve())
    found = shutil.which("metactld")
    if found:
        return str(Path(found).resolve())
    raise SystemExit(
        "metactld is not on PATH. Run `make metactld-install` first, or pass "
        "`--metactld-bin /absolute/path/to/metactld`."
    )


def server_config(args, *, cursor: bool = False) -> dict:
    config = {
        "command": resolve_metactld(args.metactld_bin),
        "args": [*SERVER_ARGS, str(args.library_root)],
        "env": {},
    }
    if cursor:
        config["type"] = "stdio"
    return config


def merge_mcp_json(path: Path, config: dict, *, force: bool) -> None:
    payload = load_json(path)
    if not isinstance(payload, dict):
        raise SystemExit(f"{path} is not a JSON object")
    servers = payload.setdefault("mcpServers", {})
    if not isinstance(servers, dict):
        raise SystemExit(f"{path} has non-object mcpServers")
    existing = servers.get(SERVER_NAME)
    if existing and existing != config and not force:
        raise SystemExit(
            f"{path} already has an unmanaged {SERVER_NAME!r} MCP server. "
            "Re-run with --force to replace it."
        )
    servers[SERVER_NAME] = config
    write_json(path, payload)
    print(f"installed {SERVER_NAME} MCP server in {path}")


def install_claude(args) -> None:
    if args.scope != "project":
        raise SystemExit("claude-code direct file install supports --scope project only")
    merge_mcp_json(args.project_root / ".mcp.json", server_config(args), force=args.force)


def install_cursor(args) -> None:
    if args.scope == "project":
        path = args.project_root / ".cursor" / "mcp.json"
    elif args.scope == "user":
        path = Path.home() / ".cursor" / "mcp.json"
    else:
        raise SystemExit("cursor supports --scope project or --scope user")
    merge_mcp_json(path, server_config(args, cursor=True), force=args.force)


def install_gemini(args) -> None:
    if args.scope == "project":
        path = args.project_root / ".gemini" / "settings.json"
    elif args.scope == "user":
        path = Path.home() / ".gemini" / "settings.json"
    else:
        raise SystemExit("gemini-cli supports --scope project or --scope user")
    merge_mcp_json(path, server_config(args), force=args.force)


def toml_string(value: str) -> str:
    return json.dumps(value)


def codex_block(config: dict) -> str:
    args = ", ".join(toml_string(arg) for arg in config["args"])
    return (
        f"{BEGIN_MARKER}\n"
        f"[mcp_servers.{SERVER_NAME}]\n"
        f"command = {toml_string(config['command'])}\n"
        f"args = [{args}]\n"
        'default_tools_approval_mode = "prompt"\n'
        f"{END_MARKER}\n"
    )


def replace_managed_block(raw: str, block: str) -> str:
    start = raw.find(BEGIN_MARKER)
    if start == -1:
        return raw.rstrip() + "\n\n" + block
    end = raw.find(END_MARKER, start)
    if end == -1:
        raise SystemExit("Found metactl MCP begin marker without end marker")
    end += len(END_MARKER)
    return raw[:start].rstrip() + "\n\n" + block + raw[end:].lstrip("\n")


def install_codex(args) -> None:
    if args.scope != "user":
        raise SystemExit("codex-cli MCP config is documented for --scope user")
    path = Path.home() / ".codex" / "config.toml"
    raw = path.read_text(encoding="utf-8") if path.exists() else ""
    if f"[mcp_servers.{SERVER_NAME}]" in raw and BEGIN_MARKER not in raw and not args.force:
        raise SystemExit(
            f"{path} already contains [mcp_servers.{SERVER_NAME}]. "
            "Re-run with --force after reviewing that table."
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        replace_managed_block(raw, codex_block(server_config(args))),
        encoding="utf-8",
    )
    print(f"installed {SERVER_NAME} MCP server in {path}")


INSTALLERS = {
    "claude-code": install_claude,
    "cursor": install_cursor,
    "codex-cli": install_codex,
    "gemini-cli": install_gemini,
}


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Install the local read-only metactl MCP server into agent config."
    )
    parser.add_argument("client", choices=sorted(INSTALLERS))
    parser.add_argument(
        "--scope",
        choices=["project", "user"],
        default=None,
        help="Config scope. Defaults to project, except codex-cli which uses user.",
    )
    parser.add_argument("--project-root", type=Path, default=ROOT)
    parser.add_argument("--library-root", type=Path, default=DEFAULT_LIBRARY_ROOT)
    parser.add_argument("--metactld-bin")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    args.project_root = args.project_root.expanduser().resolve()
    args.library_root = args.library_root.expanduser().resolve()
    if args.scope is None:
        args.scope = "user" if args.client == "codex-cli" else "project"
    if not args.library_root.exists():
        raise SystemExit(f"library root does not exist: {args.library_root}")
    INSTALLERS[args.client](args)


if __name__ == "__main__":
    try:
        main()
    except BrokenPipeError:
        sys.exit(1)
