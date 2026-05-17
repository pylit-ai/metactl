#!/usr/bin/env python3
from __future__ import annotations

import filecmp
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CANONICAL = ROOT / "library" / "starter"
PACKAGED = ROOT / "crates" / "metactl" / "assets" / "starter"


def rel_files(root: Path) -> set[Path]:
    return {path.relative_to(root) for path in root.rglob("*") if path.is_file()}


def main() -> None:
    if not CANONICAL.exists():
        raise SystemExit(f"missing canonical starter library: {CANONICAL}")
    if not PACKAGED.exists():
        raise SystemExit(f"missing packaged starter mirror: {PACKAGED}")

    canonical_files = rel_files(CANONICAL)
    packaged_files = rel_files(PACKAGED)
    missing = sorted(canonical_files - packaged_files)
    extra = sorted(packaged_files - canonical_files)
    changed = sorted(
        rel
        for rel in canonical_files & packaged_files
        if not filecmp.cmp(CANONICAL / rel, PACKAGED / rel, shallow=False)
    )

    if missing or extra or changed:
        lines = ["packaged starter mirror differs from library/starter"]
        if missing:
            lines.append("missing from packaged mirror:")
            lines.extend(f"  {item}" for item in missing)
        if extra:
            lines.append("extra in packaged mirror:")
            lines.extend(f"  {item}" for item in extra)
        if changed:
            lines.append("content differs:")
            lines.extend(f"  {item}" for item in changed)
        raise SystemExit("\n".join(lines))

    print("verify-packaged-starter-mirror: OK")


if __name__ == "__main__":
    main()
