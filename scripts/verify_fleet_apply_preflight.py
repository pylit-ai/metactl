#!/usr/bin/env python3
"""Cross-PR user path: controller member, two targets, denied access, retry.

Run against the combined fleet-preview and apply-preflight changes. This fixture
uses only disposable projects and a disposable user configuration directory.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def snapshot(root):
    entries = {}
    for path in [root, *sorted(root.rglob("*"))]:
        stat = path.lstat()
        data = (os.readlink(path) if path.is_symlink() else
                path.read_bytes() if path.is_file() else None)
        entries[str(path.relative_to(root))] = (stat.st_mode, data)
    return entries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    binary = parser.parse_args().binary.resolve(strict=True)
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory) / "project"
        home = Path(directory) / "home"
        root.mkdir()
        home.mkdir()
        (root / "metactl.yaml").write_text(
            "api_version: metactl/v2alpha1\nrole: builder\n"
            "policy: brownfield-safe-builder\ntargets: [codex-cli, claude-code]\n"
            "linked_projects:\n- {id: controller, path: .}\n"
        )
        env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"))
        for name in ["METACTL_PROFILE", "METACTL_FLEET_CONTROLLER"]:
            env.pop(name, None)

        def run(*args):
            return subprocess.run([str(binary), "--project", str(root), "--json", *args],
                                  env=env, capture_output=True, timeout=90)

        denied = root / ".claude/skills"
        denied.mkdir(parents=True)
        denied.chmod(0o500)
        try:
            try:
                (denied / "privilege-probe").write_text("probe")
            except PermissionError:
                pass
            else:
                raise AssertionError("Runner bypasses permission fixture; run as ordinary user")
            before = snapshot(root)
            preview = run("fleet", "sync", "--preview")
            assert preview.returncode == 0, preview
            assert snapshot(root) == before, "preview changed project paths"
            failed = run("--yes", "--no-input", "fleet", "sync", "--apply")
            assert failed.returncode != 0, failed
            assert b"preflight" in failed.stdout, failed
            for name in ["AGENTS.md", "CLAUDE.md", ".metactl/state/codex-cli.json",
                         ".metactl/state/claude-code.json", ".metactl/state/operation.lock"]:
                assert not (root / name).exists(), name
        finally:
            denied.chmod(0o700)
        retry = run("--yes", "--no-input", "fleet", "sync", "--apply")
        assert retry.returncode == 0, retry
        assert (root / "AGENTS.md").is_file()
        assert (root / "CLAUDE.md").is_file()
        before = snapshot(root)
        repeat_preview = run("fleet", "sync", "--preview")
        assert repeat_preview.returncode == 0, repeat_preview
        assert snapshot(root) == before, "managed preview changed project paths"
        assert not (root / ".metactl/state/operation.lock").exists()
    print("Cross-PR controller/two-target denial, retry, and immutable previews: PASS")


if __name__ == "__main__":
    main()
