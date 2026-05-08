#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LIBRARY_ROOT = ROOT / "library" / "starter"


PATH_TRAVERSAL_REQUEST = {
    "jsonrpc": "2.0",
    "id": "path-traversal",
    "method": "tools/call",
    "params": {
        "name": "metactl_compile_preview",
        "arguments": {
            "resolve_graph": {},
            "target_capability": {},
            "apply_mode": "copy",
            "project_root": "../../private",
        },
    },
}


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
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
    finally:
        request_path.unlink(missing_ok=True)

    if proc.returncode != 0:
        raise SystemExit(
            f"verify-mcp-adversarial: FAIL metactld exited {proc.returncode}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as err:
        raise SystemExit(
            f"verify-mcp-adversarial: FAIL invalid JSON: {err}\n{proc.stdout}"
        ) from err


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify MCP adversarial requests use --once request files and stay bounded."
    )
    parser.add_argument("--metactld-bin", type=Path, default=ROOT / "target" / "debug" / "metactld")
    parser.add_argument("--library-root", type=Path, default=DEFAULT_LIBRARY_ROOT)
    args = parser.parse_args()

    metactld = args.metactld_bin.expanduser().resolve()
    library_root = args.library_root.expanduser().resolve()
    if not metactld.exists():
        raise SystemExit(
            f"verify-mcp-adversarial: FAIL missing metactld: {metactld}\n"
            "Run `cargo build -p metactld` first."
        )
    if not library_root.exists():
        raise SystemExit(f"verify-mcp-adversarial: FAIL missing library root: {library_root}")

    response = invoke_once(metactld, library_root, PATH_TRAVERSAL_REQUEST)
    rendered = json.dumps(response, sort_keys=True)
    if response.get("error", {}).get("code") != -32602:
        raise SystemExit(f"verify-mcp-adversarial: FAIL expected -32602 error\n{rendered}")
    if "path traversal" not in rendered:
        raise SystemExit(f"verify-mcp-adversarial: FAIL missing traversal diagnostic\n{rendered}")
    if len(rendered) > 4096:
        raise SystemExit(f"verify-mcp-adversarial: FAIL error response too large: {len(rendered)}")

    print("verify-mcp-adversarial: OK")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BrokenPipeError:
        sys.exit(1)
