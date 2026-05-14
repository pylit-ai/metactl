#!/usr/bin/env python3
"""Install a pre-commit hook for an agent candidate library."""

from __future__ import annotations

import argparse
import os
import shlex
import stat
import subprocess
import sys
from pathlib import Path


MARKER = "metactl-agent-candidate-library-hook"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Install a metactl agent-candidate pre-commit hook."
    )
    parser.add_argument(
        "--repo",
        default=".",
        help="Git repo to install into. Defaults to the current directory.",
    )
    parser.add_argument(
        "--verify-command",
        help=(
            "Command the hook runs from the repo root. If omitted, defaults to "
            "'python3 scripts/verify_candidates.py' when that file exists."
        ),
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Replace an existing non-metactl pre-commit hook.",
    )
    return parser.parse_args()


def run_git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        message = result.stderr.strip() or result.stdout.strip()
        raise SystemExit(f"git {' '.join(args)} failed: {message}")
    return result.stdout.strip()


def resolve_repo(path: str) -> Path:
    repo = Path(path).expanduser().resolve()
    run_git(repo, "rev-parse", "--show-toplevel")
    return Path(run_git(repo, "rev-parse", "--show-toplevel")).resolve()


def resolve_hook_path(repo: Path) -> Path:
    hook_path = run_git(repo, "rev-parse", "--git-path", "hooks/pre-commit")
    path = Path(hook_path)
    if not path.is_absolute():
        path = repo / path
    return path.resolve()


def default_verify_command(repo: Path) -> str:
    candidate = repo / "scripts" / "verify_candidates.py"
    if candidate.is_file():
        return "python3 scripts/verify_candidates.py"
    raise SystemExit(
        "no --verify-command provided and scripts/verify_candidates.py was not found"
    )


def hook_body(repo: Path, verify_command: str) -> str:
    quoted_repo = shlex.quote(str(repo))
    quoted_command = shlex.quote(verify_command)
    return f"""#!/usr/bin/env sh
# {MARKER}
set -eu

REPO_ROOT={quoted_repo}
VERIFY_COMMAND={quoted_command}

cd "$REPO_ROOT"
echo "[metactl] running agent candidate verifier"
sh -c "$VERIFY_COMMAND"
"""


def install_hook(hook_path: Path, body: str, force: bool) -> None:
    if hook_path.exists():
        existing = hook_path.read_text(encoding="utf-8", errors="replace")
        if MARKER not in existing and not force:
            raise SystemExit(
                f"{hook_path} already exists; rerun with --force to replace it"
            )
    hook_path.parent.mkdir(parents=True, exist_ok=True)
    hook_path.write_text(body, encoding="utf-8")
    mode = hook_path.stat().st_mode
    hook_path.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def main() -> int:
    args = parse_args()
    repo = resolve_repo(args.repo)
    verify_command = args.verify_command or default_verify_command(repo)
    hook_path = resolve_hook_path(repo)
    install_hook(hook_path, hook_body(repo, verify_command), args.force)
    print(f"installed {hook_path}")
    print(f"verifier: {verify_command}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
