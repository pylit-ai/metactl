#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CURRENT_VERSION_DOCS = [
    "README.md",
    "docs/user/GETTING_STARTED.md",
    "docs/release-readiness.md",
]


def read(path: str) -> str:
    return (ROOT / path).read_text()


def package_version(path: str) -> str:
    match = re.search(r'^version\s*=\s*"([^"]+)"', read(path), re.MULTILINE)
    if not match:
        raise SystemExit(f"verify-version-consistency: FAIL missing version in {path}")
    return match.group(1)


def metactld_dependency_version() -> str:
    match = re.search(
        r'^metactl\s*=\s*\{\s*version\s*=\s*"([^"]+)"',
        read("crates/metactld/Cargo.toml"),
        re.MULTILINE,
    )
    if not match:
        raise SystemExit(
            "verify-version-consistency: FAIL missing metactl dependency version "
            "in crates/metactld/Cargo.toml"
        )
    return match.group(1)


def cargo_lock_version(package_name: str) -> str:
    text = read("Cargo.lock")
    package_re = re.compile(r"\[\[package\]\]\n(?P<body>.*?)(?=\n\[\[package\]\]|\Z)", re.DOTALL)
    for package in package_re.finditer(text):
        body = package.group("body")
        name = re.search(r'^name\s*=\s*"([^"]+)"', body, re.MULTILINE)
        version = re.search(r'^version\s*=\s*"([^"]+)"', body, re.MULTILINE)
        if name and version and name.group(1) == package_name:
            return version.group(1)
    raise SystemExit(f"verify-version-consistency: FAIL missing {package_name} in Cargo.lock")


def main() -> None:
    expected = package_version("crates/metactl/Cargo.toml")
    failures: list[str] = []

    checks = {
        "crates/metactld/Cargo.toml package": package_version("crates/metactld/Cargo.toml"),
        "crates/metactld/Cargo.toml metactl dependency": metactld_dependency_version(),
        "Cargo.lock metactl package": cargo_lock_version("metactl"),
        "Cargo.lock metactld package": cargo_lock_version("metactld"),
    }
    for label, actual in checks.items():
        if actual != expected:
            failures.append(f"{label}: expected {expected}, found {actual}")

    version_re = re.compile(r"\b0\.1\.\d+\b")
    for doc in CURRENT_VERSION_DOCS:
        for line_no, line in enumerate(read(doc).splitlines(), start=1):
            for match in version_re.finditer(line):
                if match.group(0) != expected:
                    failures.append(
                        f"{doc}:{line_no}: expected {expected}, found {match.group(0)}"
                    )

    if failures:
        raise SystemExit("verify-version-consistency: FAIL\n" + "\n".join(failures))

    print(f"verify-version-consistency: OK ({expected})")


if __name__ == "__main__":
    main()
