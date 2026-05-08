#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHARTER = ROOT / "docs" / "v1" / "charter.md"

REQUIRED_PHRASES = [
    "deterministic resolver/compiler/validator",
    "private-by-default",
    "0..N pinned read-only baseline libraries selected by active project/profile",
    "exactly one writable overlay per active profile",
    "generated project projections",
    "anti-bloat",
    "baseline",
    "overlay",
    "profile",
    "projection",
    "public example",
    "sanitized export",
]

REFERENCE_PATHS = [
    ROOT / "README.md",
    ROOT / "docs" / "architecture.md",
]

SEARCH_ROOTS = [
    ROOT / "docs",
    ROOT / "crates",
    ROOT / "contracts",
    ROOT / "library",
]

STALE_PATTERNS = [
    re.compile(r"\bone read-only baseline\b", re.IGNORECASE),
    re.compile(r"\bone baseline\b", re.IGNORECASE),
]

HISTORY_ALLOWLIST = (
    "migration",
    "history",
    "retrospective",
    "metactlv0",
)


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return ""


def fail(message: str) -> None:
    print(f"verify-v1-charter: FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)


def check_charter() -> None:
    if not CHARTER.exists():
        fail(f"missing {CHARTER.relative_to(ROOT)}")
    body = read(CHARTER)
    missing = [phrase for phrase in REQUIRED_PHRASES if phrase not in body]
    if missing:
        fail(
            f"{CHARTER.relative_to(ROOT)} missing required phrase(s): "
            + ", ".join(missing)
        )
    if "What metactl is" not in body or "What metactl is not" not in body:
        fail(f"{CHARTER.relative_to(ROOT)} must include is/is-not scope sections")


def check_references() -> None:
    missing_refs = [
        path.relative_to(ROOT).as_posix()
        for path in REFERENCE_PATHS
        if "docs/v1/charter.md" not in read(path)
    ]
    if missing_refs:
        fail("charter is not referenced from " + ", ".join(missing_refs))


def stale_wording_hits() -> list[str]:
    hits: list[str] = []
    for root in SEARCH_ROOTS:
        if not root.exists():
            continue
        for path in root.rglob("*"):
            if not path.is_file():
                continue
            rel = path.relative_to(ROOT).as_posix()
            if rel.startswith("target/") or rel.startswith("tmp/"):
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
            for line_no, line in enumerate(text.splitlines(), start=1):
                lower = line.lower()
                if any(marker in lower for marker in HISTORY_ALLOWLIST):
                    continue
                if any(pattern.search(line) for pattern in STALE_PATTERNS):
                    hits.append(f"{rel}:{line_no}: {line.strip()}")
    return hits


def main() -> None:
    check_charter()
    check_references()
    hits = stale_wording_hits()
    if hits:
        fail("stale baseline wording found:\n" + "\n".join(hits))
    print("verify-v1-charter: OK")


if __name__ == "__main__":
    main()
