#!/usr/bin/env python3
from __future__ import annotations

import re
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ADR = ROOT / "docs" / "v1" / "decisions" / "private-by-default-sanitized-export.md"

DENY_PATTERNS = [
    re.compile(r"private_source\s*[:=]\s*true", re.IGNORECASE),
    re.compile(r"https?://(?:[^/\s]+\.)?(?:internal|corp|private)\.", re.IGNORECASE),
    re.compile(r"/Users/(?!example\b)[A-Za-z0-9_.-]+/"),
    re.compile(r"/home/(?!example\b)[A-Za-z0-9_.-]+/"),
    re.compile(r"\b(?:sk|ghp|pat|xox[baprs])_[A-Za-z0-9_=-]{16,}\b"),
    re.compile(r"\bprivate[-_ ]?kb\b", re.IGNORECASE),
    re.compile(r"\bcustomer[-_ ]?name\b", re.IGNORECASE),
    re.compile(r"\bproprietary[-_ ]?repo[-_ ]?path\b", re.IGNORECASE),
]

ADR_REQUIRED = [
    "private-by-default",
    "sanitized_export",
    "public_example_library",
    "metactlv0",
    "broad configurability",
    "reviewer-ready diff",
]


def scan_text(text: str) -> list[str]:
    return [pattern.pattern for pattern in DENY_PATTERNS if pattern.search(text)]


def scan_tree(root: Path) -> list[str]:
    hits: list[str] = []
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        matched = scan_text(text)
        if matched:
            hits.append(f"{path.relative_to(root)}: {', '.join(matched)}")
    return hits


def fail(message: str) -> None:
    raise SystemExit(f"verify-public-boundary: FAIL: {message}")


def check_adr() -> None:
    if not ADR.exists():
        fail(f"missing {ADR.relative_to(ROOT)}")
    body = ADR.read_text(encoding="utf-8")
    missing = [phrase for phrase in ADR_REQUIRED if phrase not in body]
    if missing:
        fail(f"{ADR.relative_to(ROOT)} missing required phrase(s): {', '.join(missing)}")


def check_self_tests() -> None:
    user_path = "/" + "Users" + "/example-private/src/internal/project"
    home_path = "/" + "home" + "/example-private/src/internal/project"
    unsafe = "\n".join(
        [
            "private_source: true",
            "See https://internal.example.invalid/runbook",
            f"Path {user_path}",
            f"Home {home_path}",
            "Token sk_test_1234567890abcdef1234567890abcdef",
            "private_kb: mcp://private-kb/release-policy",
            "customer_name: Example Co",
            f"proprietary_repo_path: {user_path}",
        ]
    )
    safe = "\n".join(
        [
            "# Safe example",
            "This fixture is public example content.",
            "Docs may use /Users/example or /home/example placeholders.",
        ]
    )
    if scan_text(safe):
        fail("safe public example self-test was rejected")
    if not scan_text(unsafe):
        fail("unsafe private export self-test was not rejected")


def check_fixture_scan() -> None:
    with tempfile.TemporaryDirectory(prefix="metactl-public-boundary-") as tmp:
        root = Path(tmp)
        (root / "safe").mkdir()
        (root / "unsafe").mkdir()
        (root / "safe" / "SKILL.md").write_text(
            "---\nname: safe-example\ndescription: Public example.\n---\n",
            encoding="utf-8",
        )
        (root / "unsafe" / "SKILL.md").write_text(
            "---\nname: unsafe-export\ndescription: private_source: true\n---\n",
            encoding="utf-8",
        )
        safe_hits = scan_tree(root / "safe")
        unsafe_hits = scan_tree(root / "unsafe")
        if safe_hits:
            fail("safe fixture produced hits: " + "; ".join(safe_hits))
        if not unsafe_hits:
            fail("unsafe fixture produced no hits")


def main() -> None:
    check_adr()
    check_self_tests()
    check_fixture_scan()
    print("verify-public-boundary: OK")


if __name__ == "__main__":
    main()
