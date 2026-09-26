#!/usr/bin/env python3
"""Verify and exercise a release archive outside its source checkout."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+", args.version):
        parser.error("version must be a stable semantic version")
    archive = args.archive.resolve()
    allowed_names = {
        f"metactl-v{args.version}-{target}.tar.gz"
        for target in ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu")
    }
    if archive.name not in allowed_names:
        raise ValueError("archive name does not match release version/support matrix")
    sidecar = archive.with_suffix(archive.suffix + ".sha256").read_text().split()
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if sidecar != [digest, archive.name]:
        raise ValueError("archive checksum sidecar mismatch")
    prefix = archive.name.removesuffix(".tar.gz")
    payloads = ("metactl", "metactld", "README.md", "LICENSE", "NOTICE")
    with tempfile.TemporaryDirectory(prefix="metactl-release-smoke-") as scratch:
        root = Path(scratch)
        binaries = root / "bin"
        binaries.mkdir()
        with tarfile.open(archive, "r:gz") as tar:
            members = tar.getmembers()
            expected = {prefix, *(f"{prefix}/{name}" for name in payloads)}
            if len(members) != len(expected) or {m.name for m in members} != expected:
                raise ValueError("unexpected or duplicate archive members")
            for member in members:
                if member.name == prefix:
                    if not member.isdir():
                        raise ValueError("archive root is not a directory")
                    continue
                if not member.isfile():
                    raise ValueError("release payload must be a regular file")
                name = member.name.split("/")[1]
                contents = tar.extractfile(member)
                if contents is None:
                    raise ValueError("missing release payload")
                destination = binaries / name
                destination.write_bytes(contents.read())
                destination.chmod(0o755 if name in payloads[:2] else 0o644)
        project = root / "project"
        project.mkdir()
        home = root / "home"
        home.mkdir()
        empty_config = root / "empty-git-config"
        empty_config.touch()
        env = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "WINDIR", "TMPDIR") if key in os.environ}
        env.update(HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
                   GIT_CONFIG_GLOBAL=str(empty_config), GIT_CONFIG_NOSYSTEM="1")

        def run(command: list[str], stdin: str | None = None) -> str:
            result = subprocess.run(command, cwd=project, env=env, input=stdin,
                                    text=True, capture_output=True, timeout=90)
            if result.returncode:
                raise RuntimeError(f"{command[0]} {command[1:]} failed: {result.stderr}\n{result.stdout}")
            return result.stdout

        cli = str(binaries / "metactl")
        daemon = str(binaries / "metactld")
        assert run([cli, "--version"]).strip() == f"metactl {args.version}"
        assert run([daemon, "--version"]).strip() == f"metactld {args.version}"
        run(["git", "init", "--quiet"])
        run([cli, "init", "-t", "codex-cli", "--no-input"])
        run([cli, "use", "python-refactor", "--no-input"])
        assert (project / "AGENTS.md").is_file()
        json.loads(run([cli, "sync", "--preview", "--json", "--no-input"]))
        run([cli, "validate", "--no-input"])
        status = json.loads(run([cli, "skills", "host", "--status"]))
        assert status["project_ready"] and not status["provider_verified"]
        request = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}) + "\n"
        response = json.loads(run([cli, "skills", "host", "--ranker", "deterministic"], request))
        assert response.get("id") == 1 and "error" not in response
        assert any(tool["name"] == "discover_skills" for tool in response["result"]["tools"])
    print(json.dumps({"status": "pass", "version": args.version,
                      "archive": archive.name, "sha256": digest,
                      "checks": ["checksum", "archive members", "both binary versions",
                                 "fresh init/use/sync/validate", "offline host status", "MCP tools/list"]}))


if __name__ == "__main__":
    main()

