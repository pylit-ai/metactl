#!/usr/bin/env python3
from __future__ import annotations

import re
from pathlib import Path
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[1]
CHECKED = [ROOT / "README.md", ROOT / "docs" / "v1"]
LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")


def iter_markdown() -> list[Path]:
    paths: list[Path] = []
    for item in CHECKED:
        if item.is_file():
            paths.append(item)
        elif item.is_dir():
            paths.extend(sorted(item.rglob("*.md")))
    return paths


def is_external(target: str) -> bool:
    return target.startswith(("http://", "https://", "mailto:", "#"))


def resolve_link(source: Path, target: str) -> Path:
    path_part = target.split("#", 1)[0]
    path_part = unquote(path_part)
    if not path_part:
        return source
    return (source.parent / path_part).resolve()


def main() -> None:
    failures: list[str] = []
    for source in iter_markdown():
        text = source.read_text(encoding="utf-8")
        for line_no, line in enumerate(text.splitlines(), start=1):
            if line.lstrip().startswith("!"):
                continue
            for match in LINK_RE.finditer(line):
                target = match.group(1).strip()
                if not target or is_external(target):
                    continue
                resolved = resolve_link(source, target)
                try:
                    resolved.relative_to(ROOT)
                except ValueError:
                    failures.append(f"{source.relative_to(ROOT)}:{line_no}: escapes repo: {target}")
                    continue
                if not resolved.exists():
                    failures.append(f"{source.relative_to(ROOT)}:{line_no}: missing link target: {target}")
    if failures:
        raise SystemExit("verify-docs-links: FAIL\n" + "\n".join(failures))
    print("verify-docs-links: OK")


if __name__ == "__main__":
    main()
