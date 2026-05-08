#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]

REQUIRED_PATHS = {
    "PYL-355": ["docs/v1/charter.md", "scripts/verify_v1_charter.py"],
    "PYL-356": ["docs/v1/decisions/private-by-default-sanitized-export.md", "scripts/verify_public_boundary.py"],
    "PYL-357": ["contracts/schemas/metactl/starter_library_manifest.schema.json", "contracts/schemas/metactl/target_capability_matrix.schema.json"],
    "PYL-360": ["contracts/schemas/metactl/knowledge_source_manifest.schema.json", "fixtures/knowledge_sources/filesystem-markdown.json", "fixtures/knowledge_sources/llms-txt-index.json", "fixtures/knowledge_sources/mcp-resource.json"],
    "PYL-362": ["contracts/schemas/metactl/library_stack_manifest.schema.json", "contracts/schemas/metactl/library_source_manifest.schema.json", "contracts/schemas/metactl/library_profile_manifest.schema.json", "contracts/schemas/metactl/library_stack_lock.schema.json", "fixtures/library_stack/user-only/stack.json", "fixtures/library_stack/locked-conflict/stack.json"],
    "PYL-363": ["crates/metactl/src/library_stack.rs"],
    "PYL-366": ["library/starter/packs/release-manager.json", "library/starter/packs/release-manager/SKILL.md"],
    "PYL-369": ["contracts/schemas/metactl/conformance_matrix.schema.json", "fixtures/v1/conformance.matrix.json", "docs/v1/conformance.md"],
    "PYL-371": ["contracts/schemas/metactl/sanitized_export.schema.json", "fixtures/v1/sanitized-export.sample.json", "docs/v1/sanitized-export.md"],
    "PYL-372": ["docs/v1/onboarding.md", "docs/v1/migration.md"],
}

COMMAND_GATES = [
    ["cargo", "build", "-p", "metactl", "-p", "metactld"],
    [sys.executable, "scripts/verify_v1_charter.py"],
    ["bash", "scripts/check_public_boundary.sh"],
    [sys.executable, "scripts/verify_public_boundary.py"],
    [sys.executable, "scripts/verify_docs_links.py"],
    [sys.executable, "scripts/verify_docs_commands.py"],
    [sys.executable, "scripts/verify_mcp_adversarial.py"],
    [
        sys.executable,
        "scripts/validate_contracts.py",
        "--include-starter-library",
        "--include-targets",
        "--include-knowledge-fixtures",
        "--library-stack-fixtures",
    ],
]

COVERAGE_CHECKS = [
    {
        "id": "agent-skills-commands",
        "ticket": "PYL-364",
        "description": "Pack/Agent Skills import, export, and verify command surface",
        "patterns": [
            ("crates/metactl/src/main.rs", r"import-skill"),
            ("crates/metactl/src/main.rs", r"export-skill"),
            ("crates/metactl/src/main.rs", r"verify-skill"),
        ],
    },
    {
        "id": "agent-skills-negative-fixtures",
        "ticket": "PYL-364",
        "description": "Agent Skills import-safety negative fixture coverage",
        "patterns": [
            ("fixtures", r"hidden secret|hidden_secret|symlink escape|symlink_escape|path traversal|path_traversal|oversized|malformed frontmatter|executable script"),
        ],
    },
    {
        "id": "mcp-security-fixtures",
        "ticket": "PYL-367",
        "description": "Read-only MCP security fixtures for traversal, redaction, oversized output, malicious JSON-RPC, and untrusted KB content",
        "patterns": [
            ("crates/metactl/tests", r"path traversal|path_traversal"),
            ("crates/metactl/tests", r"secret redaction|redact"),
            ("crates/metactl/tests", r"oversized"),
            ("crates/metactl/tests", r"malicious|unauthorized"),
            ("crates/metactl/tests", r"untrusted KB|untrusted_kb|tool descriptions"),
        ],
    },
    {
        "id": "committed-projection-fixtures",
        "ticket": "PYL-368",
        "description": "Projection drift and committed-projection profile fixture coverage",
        "patterns": [
            ("contracts/schemas/metactl/library_profile_manifest.schema.json", r"committed_projection"),
            ("crates/metactl/tests", r"committed_projection"),
            ("crates/metactl/tests", r"stale lockfile|stale_lockfile"),
        ],
    },
    {
        "id": "knowledge-freshness-provenance",
        "ticket": "PYL-365",
        "description": "KnowledgeSource freshness, provenance, stable-code, supersession, and digest-change coverage",
        "patterns": [
            ("crates/metactl/src/main.rs", r"METACTL_KS_EXPIRED_WARN"),
            ("crates/metactl/src/main.rs", r"METACTL_KS_EXPIRED_IGNORE"),
            ("crates/metactl/src/main.rs", r"METACTL_KS_SUPERSEDED"),
            ("scripts/validate_contracts.py", r"METACTL_KS_MISSING_OWNER"),
            ("fixtures/library_stack", r"METACTL_STACK_SOURCE_DIGEST_CHANGED"),
            ("fixtures/library_stack", r"x-provenance"),
            ("fixtures/library_stack", r"x-freshness"),
        ],
    },
    {
        "id": "sanitized-export-commands",
        "ticket": "PYL-371",
        "description": "Public example, sanitized export, and public-boundary CLI command equivalents",
        "patterns": [
            ("crates/metactl/src/main.rs", r"public-example"),
            ("crates/metactl/src/main.rs", r"export sanitized|sanitized"),
            ("crates/metactl/src/main.rs", r"check-public-boundary"),
        ],
    },
    {
        "id": "v1-json-report-gate",
        "ticket": "PYL-373",
        "description": "Top-level v1 gate target and JSON report script",
        "patterns": [
            ("Makefile", r"verify-v1-lightweight-control-plane"),
            ("scripts/verify_v1_lightweight_control_plane.py", r"v1_lightweight_control_plane_report"),
        ],
    },
]


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return ""


def path_check(ticket: str, paths: list[str]) -> dict[str, Any]:
    missing = [item for item in paths if not (ROOT / item).exists()]
    return {
        "id": f"{ticket.lower()}-required-paths",
        "ticket": ticket,
        "description": "Required files exist",
        "status": "pass" if not missing else "fail",
        "evidence": [item for item in paths if (ROOT / item).exists()],
        "missing": missing,
    }


def pattern_exists(rel: str, pattern: str) -> bool:
    base = ROOT / rel
    regex = re.compile(pattern, re.IGNORECASE)
    if base.is_file():
        return bool(regex.search(read_text(base)))
    if base.is_dir():
        for child in base.rglob("*"):
            if child.is_file() and regex.search(read_text(child)):
                return True
    return False


def coverage_check(spec: dict[str, Any]) -> dict[str, Any]:
    missing = []
    evidence = []
    for rel, pattern in spec["patterns"]:
        if pattern_exists(rel, pattern):
            evidence.append(f"{rel} matches /{pattern}/")
        else:
            missing.append(f"{rel} lacks /{pattern}/")
    return {
        "id": spec["id"],
        "ticket": spec["ticket"],
        "description": spec["description"],
        "status": "pass" if not missing else "fail",
        "evidence": evidence,
        "missing": missing,
    }


def run_command(command: list[str]) -> dict[str, Any]:
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
    elapsed = round(time.monotonic() - started, 3)
    output = "\n".join(part for part in [result.stdout, result.stderr] if part).strip()
    return {
        "id": "command:" + " ".join(command),
        "description": " ".join(command),
        "status": "pass" if result.returncode == 0 else "fail",
        "returncode": result.returncode,
        "runtime_seconds": elapsed,
        "output_tail": output[-4000:],
    }


def fixture_list() -> list[str]:
    roots = ["fixtures/v1", "fixtures/library_stack", "fixtures/knowledge_sources"]
    items: list[str] = []
    for rel in roots:
        base = ROOT / rel
        if base.exists():
            items.extend(str(path.relative_to(ROOT)) for path in sorted(base.rglob("*")) if path.is_file())
    return items


def conformance_summary() -> dict[str, Any]:
    path = ROOT / "fixtures/v1/conformance.matrix.json"
    if not path.exists():
        return {"path": str(path.relative_to(ROOT)), "target_count": 0, "targets": []}
    data = json.loads(path.read_text(encoding="utf-8"))
    targets = data.get("targets", [])
    return {
        "path": str(path.relative_to(ROOT)),
        "target_count": len(targets),
        "targets": [target.get("target_id") for target in targets],
    }


def git_head() -> str | None:
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, capture_output=True)
    if result.returncode == 0:
        return result.stdout.strip()
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", default="tmp/v1-lightweight-control-plane-report.json")
    args = parser.parse_args()

    started = time.monotonic()
    checks: list[dict[str, Any]] = []
    checks.extend(path_check(ticket, paths) for ticket, paths in REQUIRED_PATHS.items())
    checks.extend(coverage_check(spec) for spec in COVERAGE_CHECKS)
    checks.extend(run_command(command) for command in COMMAND_GATES)

    failures = [check for check in checks if check.get("status") != "pass"]
    report = {
        "kind": "v1_lightweight_control_plane_report",
        "generated_at": datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
        "runtime_seconds": round(time.monotonic() - started, 3),
        "status": "pass" if not failures else "fail",
        "checks": checks,
        "failures": failures,
        "warnings": [],
        "fixture_list": fixture_list(),
        "conformance_matrix": conformance_summary(),
        "provenance_summary": {
            "git_head": git_head(),
            "root": str(ROOT),
            "network_required": False,
            "credentials_required": False,
        },
    }

    report_path = ROOT / args.report
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if failures:
        print(f"verify-v1-lightweight-control-plane: FAIL ({len(failures)} failing checks); report={report_path.relative_to(ROOT)}")
        return 1
    print(f"verify-v1-lightweight-control-plane: OK; report={report_path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
