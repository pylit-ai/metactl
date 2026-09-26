#!/usr/bin/env python3
"""Compare two fleet CLI implementations on identical disposable fixtures.

No normalization: each binary sees the same paths and bytes after fixture reset.
Outputs, return codes, file bytes/modes, and symlink destinations must match.
These cases intentionally avoid successful apply timestamps; adverse apply and
retry lifecycles are covered by fleet_adversarial.rs against both binaries.
"""
import argparse
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile


CONFIG = "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets: [codex-cli]\n"


def snapshot(root):
    result = {}
    for path in sorted(root.rglob("*")):
        info = path.lstat()
        kind = stat.S_IFMT(info.st_mode)
        content = os.readlink(path) if path.is_symlink() else path.read_bytes() if path.is_file() else None
        result[str(path.relative_to(root))] = (kind, stat.S_IMODE(info.st_mode), content)
    return result


def seed(root, scenario):
    root.mkdir()
    (root / "home").mkdir()
    (root / "ready space 界").mkdir()
    (root / "ready space 界/metactl.yaml").write_text(CONFIG)
    (root / "bad").mkdir()
    (root / "bad/metactl.yaml").write_text("linked_projects: invalid\n")
    (root / "AGENTS.md").write_text("# Preserve controller instructions\n")
    (root / "metactl.yaml").write_text(CONFIG + "linked_projects:\n- {id: ready, path: 'ready space 界'}\n- {id: bad, path: bad}\n- {id: absent, path: absent}\n- {id: disabled, path: 'ready space 界', disabled: true}\n")
    if scenario == "lock":
        state = root / ".metactl/state"
        state.mkdir(parents=True)
        (state / "operation.lock").write_text("pid=123\ncommand=sync\nstarted_at=18446744073709551615\n")
    if scenario == "bad-state":
        (root / ".metactl").mkdir()
        (root / ".metactl/state").write_text("preserve obstruction\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    args = parser.parse_args()
    binaries = [p.resolve(strict=True) for p in [args.baseline, args.candidate]]
    cases = [
        ("plain", ["fleet", "list"]),
        ("plain", ["fleet", "status"]),
        ("plain", ["fleet", "sync", "--preview"]),
        ("plain", ["fleet", "status", "--id", "unknown"]),
        ("plain", ["fleet", "sync", "--apply"]),
        ("lock", ["--yes", "--no-input", "fleet", "sync", "--apply"]),
        ("bad-state", ["--yes", "--no-input", "fleet", "sync", "--apply"]),
    ]
    count = 0
    with tempfile.TemporaryDirectory(prefix="metactl-fleet-compare-") as directory:
        root = Path(directory) / "fixture"
        for scenario, command in cases:
            for mode in [[], ["--json"], ["--agent"]]:
                observations = []
                for binary in binaries:
                    if root.exists():
                        shutil.rmtree(root)
                    seed(root, scenario)
                    before = snapshot(root)
                    env = {k: v for k, v in os.environ.items() if not k.startswith("METACTL_")}
                    env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "home/.config"), NO_COLOR="1")
                    result = subprocess.run([str(binary), "--project", str(root), *mode, *command], env=env, capture_output=True, timeout=60)
                    after = snapshot(root)
                    # Loading the bundled library can populate disposable HOME.
                    # Compare those cache bytes between binaries, but require the
                    # controller and member project trees to stay unchanged.
                    changes = [p for p in set(before) | set(after) if not p.startswith("home/") and before.get(p) != after.get(p)]
                    if "--apply" not in command:
                        assert not changes, f"unexpected filesystem mutation: {scenario} {mode} {command}: {sorted(changes)[:15]}"
                    observations.append((result.returncode, result.stdout, result.stderr, after))
                assert observations[0] == observations[1], f"behavior differs: {scenario} {mode} {command}\n{observations[0][:3]!r}\n{observations[1][:3]!r}"
                count += 1
    print(f"Fleet differential comparison: {count} exact matches; read-only cases preserve project trees")


if __name__ == "__main__":
    main()
