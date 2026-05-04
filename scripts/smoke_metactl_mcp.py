#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LIBRARY_ROOT = ROOT / "library" / "starter"
EXPECTED_TOOLS = [
    "metactl_search_packs",
    "metactl_explain",
    "metactl_compile_preview",
    "metactl_validate",
]
CURSOR_INITIALIZE_REQUEST = {
    "jsonrpc": "2.0",
    "id": "init-cursor",
    "method": "initialize",
    "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {
            "tools": True,
            "prompts": True,
            "resources": True,
            "logging": False,
            "elicitation": {},
        },
        "clientInfo": {"name": "Cursor", "version": "1.0.0"},
    },
}
SEARCH_REQUEST = {
    "jsonrpc": "2.0",
    "id": "search",
    "method": "tools/call",
    "params": {
        "name": "metactl_search_packs",
        "arguments": {
            "query": "install metactl in a brownfield repo",
            "config": {
                "api_version": "metactl/v2alpha1",
                "role": {"kind": "role", "id": "builder", "version": "1.0.0"},
                "policy": {
                    "kind": "policy",
                    "id": "brownfield-safe-builder",
                    "version": "1.0.0",
                },
                "targets": [
                    {"kind": "target", "id": "codex-cli", "version": "2026.03.26"}
                ],
            },
            "limit": 3,
        },
    },
}


def resolve_metactld(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).expanduser().resolve()
    found = shutil.which("metactld")
    if found:
        return Path(found).resolve()
    fallback = ROOT / "target" / "debug" / "metactld"
    if fallback.exists():
        return fallback
    raise SystemExit(
        "metactld is not on PATH and target/debug/metactld does not exist. "
        "Run `cargo build -p metactld` or `make metactld-install` first."
    )


def invoke_once(metactld: Path, library_root: Path, request: dict) -> dict:
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", delete=False) as handle:
        json.dump(request, handle)
        handle.write("\n")
        request_path = Path(handle.name)
    try:
        proc = subprocess.run(
            [
                str(metactld),
                "--mcp",
                "--once",
                str(request_path),
                "--library-root",
                str(library_root),
            ],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    finally:
        request_path.unlink(missing_ok=True)

    if proc.returncode != 0:
        raise SystemExit(
            f"metactld exited {proc.returncode}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    try:
        response = json.loads(proc.stdout)
    except json.JSONDecodeError as err:
        raise SystemExit(f"metactld returned invalid JSON: {err}\n{proc.stdout}") from err
    if "error" in response:
        error = response["error"]
        detail = error.get("data") or error.get("message") or error
        raise SystemExit(f"MCP request failed: {detail}")
    return response


def main() -> None:
    parser = argparse.ArgumentParser(description="Smoke-test the local metactl MCP server.")
    parser.add_argument("--metactld-bin")
    parser.add_argument("--library-root", type=Path, default=DEFAULT_LIBRARY_ROOT)
    args = parser.parse_args()

    metactld = resolve_metactld(args.metactld_bin)
    library_root = args.library_root.expanduser().resolve()
    if not library_root.exists():
        raise SystemExit(f"library root does not exist: {library_root}")

    init_response = invoke_once(metactld, library_root, CURSOR_INITIALIZE_REQUEST)
    negotiated = init_response["result"]["protocolVersion"]
    if negotiated != "2025-06-18":
        raise SystemExit(f"unexpected negotiated MCP protocol version: {negotiated}")

    tools_response = invoke_once(
        metactld,
        library_root,
        {"jsonrpc": "2.0", "id": "tools", "method": "tools/list", "params": {}},
    )
    tools = [tool["name"] for tool in tools_response["result"]["tools"]]
    if tools != EXPECTED_TOOLS:
        raise SystemExit(f"unexpected MCP tools: {tools}")

    search_response = invoke_once(metactld, library_root, SEARCH_REQUEST)
    matches = search_response["result"]["structuredContent"]["matches"]
    first_id = matches[0]["pack_ref"]["id"] if matches else None
    if first_id != "metactl-project-onboarding":
        raise SystemExit(f"unexpected first search match: {first_id}")

    print(f"ok negotiated protocol: {negotiated}")
    print(f"ok tools: {', '.join(tools)}")
    print(f"ok search first match: {first_id}")


if __name__ == "__main__":
    try:
        main()
    except BrokenPipeError:
        sys.exit(1)
