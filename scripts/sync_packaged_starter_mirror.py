#!/usr/bin/env python3
from __future__ import annotations

import shutil
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CANONICAL = ROOT / "library" / "starter"
PACKAGED = ROOT / "crates" / "metactl" / "assets" / "starter"


def main() -> None:
    if not CANONICAL.exists():
        raise SystemExit(f"missing canonical starter library: {CANONICAL}")

    PACKAGED.parent.mkdir(parents=True, exist_ok=True)
    if PACKAGED.exists():
        shutil.rmtree(PACKAGED)
    shutil.copytree(CANONICAL, PACKAGED)
    print("sync-packaged-starter-mirror: OK")


if __name__ == "__main__":
    main()
