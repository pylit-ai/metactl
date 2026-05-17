#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

REQUIRED_PATHS = [
    "docs/v1/charter.md",
    "docs/v1/decisions/private-by-default-sanitized-export.md",
    "docs/v1/onboarding.md",
    "docs/v1/migration.md",
    "docs/v1/conformance.md",
    "docs/v1/sanitized-export.md",
    "contracts/schemas/metactl/knowledge_source_manifest.schema.json",
    "contracts/schemas/metactl/library_stack_manifest.schema.json",
    "contracts/schemas/metactl/conformance_matrix.schema.json",
    "contracts/schemas/metactl/sanitized_export.schema.json",
    "fixtures/knowledge_sources/filesystem-markdown.json",
    "fixtures/library_stack/user-only/stack.json",
    "fixtures/v1/conformance.matrix.json",
    "fixtures/v1/sanitized-export.sample.json",
    "library/starter/targets/filesystem-agent.json",
    "library/starter/packs/release-manager.json",
    "crates/metactl/assets/starter/library.json",
]

COMMANDS = [
    ["cargo", "build", "-p", "metactl", "-p", "metactld"],
    [sys.executable, "scripts/verify_packaged_starter_mirror.py"],
    [sys.executable, "scripts/verify_v1_charter.py"],
    ["bash", "scripts/check_public_boundary.sh"],
    [sys.executable, "scripts/verify_public_boundary.py"],
    [sys.executable, "scripts/verify_docs_links.py"],
    [sys.executable, "scripts/verify_docs_commands.py"],
    [sys.executable, "scripts/verify_version_consistency.py"],
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


def docker_available() -> bool:
    try:
        return (
            subprocess.run(["docker", "info"], cwd=ROOT, capture_output=True).returncode
            == 0
        )
    except FileNotFoundError:
        return False


def main() -> None:
    missing = [item for item in REQUIRED_PATHS if not (ROOT / item).exists()]
    if missing:
        raise SystemExit("verify-v1-release-gate: FAIL missing paths:\n" + "\n".join(missing))
    for command in COMMANDS:
        result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
        if result.returncode != 0:
            output = "\n".join(part for part in [result.stdout, result.stderr] if part)
            raise SystemExit(
                "verify-v1-release-gate: FAIL command "
                + " ".join(command)
                + "\n"
                + output
            )
    if docker_available():
        result = subprocess.run(
            ["bash", "scripts/smoke_packaged_metactl.sh"],
            cwd=ROOT,
            text=True,
            capture_output=True,
        )
        if result.returncode != 0:
            output = "\n".join(part for part in [result.stdout, result.stderr] if part)
            raise SystemExit(
                "verify-v1-release-gate: FAIL command bash scripts/smoke_packaged_metactl.sh\n"
                + output
            )
    else:
        print("verify-v1-release-gate: SKIP packaged Docker smoke; Docker unavailable")
    print("verify-v1-release-gate: OK")


if __name__ == "__main__":
    main()
