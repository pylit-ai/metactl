#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TMP_BASE="$REPO_ROOT/tmp"
mkdir -p "$TMP_BASE"
PROJECT_ROOT="$(mktemp -d "$TMP_BASE/stdio-smoke.XXXXXX")"
export REPO_ROOT PROJECT_ROOT

python3 <<'PY'
import json
import os
import pathlib
import subprocess
import sys

repo_root = pathlib.Path(os.environ["REPO_ROOT"])
project_root = pathlib.Path(os.environ["PROJECT_ROOT"])

config = {
    "api_version": "metactl/v2alpha1",
    "role": {"kind": "role", "id": "reviewer", "version": "1.0.0"},
    "packs": [],
    "policy": {"kind": "policy", "id": "safe-review", "version": "1.0.0"},
    "targets": [{"kind": "target", "id": "openclaw", "version": "2026.03.26"}],
}
target = json.loads((repo_root / "library/starter/targets/openclaw.json").read_text())
overlay = {
    "entrypoint": "magicwormhole_notch",
    "selected_target_override": {
        "kind": "target",
        "id": "openclaw",
        "version": "2026.03.26",
    },
}

proc = subprocess.Popen(
    [
        "cargo",
        "run",
        "-p",
        "metactld",
        "--",
        "--library-root",
        str(repo_root / "library/starter"),
        "--stdio",
    ],
    cwd=repo_root,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)

def round_trip(payload):
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    if not line:
        stderr = proc.stderr.read()
        raise RuntimeError(f"metactld produced no response\n{stderr}")
    data = json.loads(line)
    if data.get("error"):
        raise RuntimeError(f"rpc error: {data['error']}")
    return data["result"]

search = round_trip({
    "jsonrpc": "2.0",
    "id": "search-1",
    "method": "metactl.search",
    "params": {
        "query": "review correctness",
        "config": config,
        "overlay": overlay,
    },
})
assert search["matches"], "expected search matches"

resolve = round_trip({
    "jsonrpc": "2.0",
    "id": "resolve-1",
    "method": "metactl.resolve",
    "params": {
        "config": config,
        "overlay": overlay,
        "available_targets": [target],
    },
})
assert resolve["selected_target"]["id"] == "openclaw"

explain = round_trip({
    "jsonrpc": "2.0",
    "id": "explain-1",
    "method": "metactl.explain",
    "params": {
        "resolve_graph": resolve,
    },
})
assert "openclaw" in explain["summary"]

compile = round_trip({
    "jsonrpc": "2.0",
    "id": "compile-1",
    "method": "metactl.compile",
    "params": {
        "resolve_graph": resolve,
        "target_capability": target,
        "apply_mode": "copy",
        "emit_policy_report": True,
        "project_root": str(project_root),
    },
})
assert compile["compile_manifest"]["generated_outputs"], "expected staged outputs"

validate = round_trip({
    "jsonrpc": "2.0",
    "id": "validate-1",
    "method": "metactl.validate",
    "params": {
        "subject_ref": target["target_ref"] if "target_ref" in target else {
            "kind": "target",
            "id": "openclaw",
            "version": "2026.03.26",
        },
        "compile_manifest": compile["compile_manifest"],
        "policy_enforcement_report": compile.get("policy_enforcement_report"),
        "project_root": str(project_root),
    },
})
assert validate["status"] in ("pass", "warn")

proc.terminate()
proc.wait(timeout=20)
PY
