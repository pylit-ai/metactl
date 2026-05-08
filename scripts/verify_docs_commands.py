#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import shlex
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CHECKED = [ROOT / "README.md", ROOT / "docs" / "v1"]
INLINE_RE = re.compile(r"`([^`\n]+)`")
FENCE_RE = re.compile(r"^```([A-Za-z0-9_-]*)\s*$")
CHECKED_BINARIES = {"metactl", "metactld"}


def iter_markdown() -> list[Path]:
    paths: list[Path] = []
    for item in CHECKED:
        if item.is_file():
            paths.append(item)
        elif item.is_dir():
            paths.extend(sorted(item.rglob("*.md")))
    return paths


def strip_prompt(line: str) -> str:
    line = line.strip()
    for prefix in ("$ ", "% "):
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
    return line


def strip_shell_wrappers(line: str) -> str:
    line = strip_prompt(line)
    for prefix in ("env ", "command "):
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
    return line


def command_candidates(source: Path) -> list[tuple[int, str]]:
    candidates: list[tuple[int, str]] = []
    in_fence = False
    fence_lang = ""
    for line_no, raw in enumerate(source.read_text(encoding="utf-8").splitlines(), start=1):
        fence_match = FENCE_RE.match(raw.strip())
        if fence_match:
            if in_fence:
                in_fence = False
                fence_lang = ""
            else:
                in_fence = True
                fence_lang = fence_match.group(1).lower()
            continue

        if in_fence:
            if fence_lang in {"", "bash", "sh", "shell", "console"}:
                line = strip_shell_wrappers(raw)
                if line.split(" ", 1)[0] in CHECKED_BINARIES:
                    candidates.append((line_no, line))
            continue

        for match in INLINE_RE.finditer(raw):
            line = strip_shell_wrappers(match.group(1))
            if line.split(" ", 1)[0] in CHECKED_BINARIES:
                candidates.append((line_no, line))
    return candidates


def load_metactl_commands(metactl_bin: Path) -> set[str]:
    result = subprocess.run(
        [str(metactl_bin), "--help"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        return set()
    commands: set[str] = set()
    in_commands = False
    for raw in result.stdout.splitlines():
        if raw.strip() == "Commands:":
            in_commands = True
            continue
        if in_commands and raw and not raw.startswith(" "):
            break
        if in_commands:
            parts = raw.strip().split()
            if parts:
                commands.add(parts[0])
    return commands


def doc_command_prefix(line: str, metactl_commands: set[str]) -> list[str] | None:
    try:
        tokens = shlex.split(line)
    except ValueError:
        return None
    if not tokens or tokens[0] not in CHECKED_BINARIES:
        return None
    if tokens[0] == "metactl" and len(tokens) > 1:
        first_arg = tokens[1]
        if not first_arg.startswith("-") and first_arg not in metactl_commands:
            return None
    if tokens[0] == "metactld" and len(tokens) > 1 and not tokens[1].startswith("-"):
        return None
    prefix = [tokens[0]]
    for token in tokens[1:]:
        if token == "--":
            break
        if token.startswith("-"):
            break
        if any(marker in token for marker in ("=", "$", "/", "\\", "*", ":", "{", "}")):
            break
        prefix.append(token)
    return prefix


def run_help(prefix: list[str], bins: dict[str, Path]) -> subprocess.CompletedProcess[str]:
    binary = bins[prefix[0]]
    return subprocess.run(
        [str(binary), *prefix[1:], "--help"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify documented metactl/metactld command references resolve to help."
    )
    parser.add_argument("--metactl-bin", type=Path, default=ROOT / "target" / "debug" / "metactl")
    parser.add_argument("--metactld-bin", type=Path, default=ROOT / "target" / "debug" / "metactld")
    args = parser.parse_args()

    bins = {
        "metactl": args.metactl_bin.expanduser().resolve(),
        "metactld": args.metactld_bin.expanduser().resolve(),
    }
    missing_bins = [name for name, path in bins.items() if not path.exists()]
    if missing_bins:
        missing = ", ".join(f"{name} at {bins[name]}" for name in missing_bins)
        raise SystemExit(
            "verify-docs-commands: FAIL missing binaries: "
            + missing
            + "\nRun `cargo build -p metactl -p metactld` first."
        )
    metactl_commands = load_metactl_commands(bins["metactl"])

    failures: list[str] = []
    checked = 0
    seen: set[tuple[str, ...]] = set()
    for source in iter_markdown():
        for line_no, line in command_candidates(source):
            prefix = doc_command_prefix(line, metactl_commands)
            if not prefix:
                continue
            key = tuple(prefix)
            if key not in seen:
                result = run_help(prefix, bins)
                seen.add(key)
                if result.returncode != 0:
                    output = "\n".join(part for part in [result.stdout, result.stderr] if part).strip()
                    failures.append(
                        f"{source.relative_to(ROOT)}:{line_no}: command help failed: "
                        + " ".join(prefix)
                        + "\n"
                        + output[-1200:]
                    )
            checked += 1

    if failures:
        raise SystemExit("verify-docs-commands: FAIL\n" + "\n".join(failures))
    print(f"verify-docs-commands: OK ({checked} references, {len(seen)} command surfaces)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BrokenPipeError:
        sys.exit(1)
