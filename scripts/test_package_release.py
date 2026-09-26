#!/usr/bin/env python3
"""Consumer-level archive checks, including metadata and rejection cases."""
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

import package_release as release


class ReleaseArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binaries = self.root / "binaries"
        self.binaries.mkdir()
        for name in release.FILES:
            parent = self.binaries if name in release.FILES[:2] else self.root
            (parent / name).write_bytes((name + "\n").encode())

    def build(self, **changes):
        args = dict(root=self.root, binaries=self.binaries, output=self.root / "dist",
                    version="0.1.21", target=release.TARGETS[0], epoch=1700000000, tag="v0.1.21")
        args.update(changes)
        return release.package(**args)

    def test_repeated_packaging_ignores_file_times_modes_and_output_directory(self):
        first = self.build().read_bytes()
        for path in [*self.binaries.iterdir(), *(self.root / n for n in release.FILES[2:])]:
            os.utime(path, (1000000000, 1000000000))
            path.chmod(0o600)
        second = self.build(output=self.root / "other location").read_bytes()
        self.assertEqual(first, second)
        self.assertEqual(second[4:8], b"\0\0\0\0")  # gzip timestamp

    def test_archive_contents_modes_ownership_and_checksum(self):
        for target in release.TARGETS:
            with self.subTest(target=target):
                archive = self.build(target=target)
                with tarfile.open(archive) as tar:
                    members = tar.getmembers()
                    self.assertEqual(len(members), 6)
                    self.assertTrue(members[0].isdir())
                    for member, name in zip(members[1:], release.FILES):
                        self.assertTrue(member.isfile())
                        self.assertEqual(member.name, f"metactl-v0.1.21-{target}/{name}")
                        self.assertEqual(tar.extractfile(member).read(), (name + "\n").encode())
                        self.assertEqual(member.mode, 0o755 if name in release.FILES[:2] else 0o644)
                    for member in members:
                        self.assertEqual((member.uid, member.gid, member.uname, member.gname), (0, 0, "", ""))
                        self.assertEqual(member.mtime, 1700000000)
                digest = hashlib.sha256(archive.read_bytes()).hexdigest()
                self.assertEqual(Path(str(archive) + ".sha256").read_text(), f"{digest}  {archive.name}\n")

    def test_changed_payload_changes_archive(self):
        before = self.build().read_bytes()
        (self.binaries / "metactl").write_bytes(b"new binary bytes")
        self.assertNotEqual(before, self.build().read_bytes())

    def test_invalid_identity_leaves_no_output(self):
        for changes in [dict(tag="v0.1.22"), dict(tag="main"), dict(version="../escape"),
                        dict(target="wrong-target"), dict(epoch=-1)]:
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                self.build(**changes)
        self.assertFalse((self.root / "dist").exists())

    def test_missing_empty_directory_and_symlink_payloads_preserve_existing_archive(self):
        archive = self.build()
        before = archive.read_bytes()
        path = self.binaries / "metactld"
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            self.build()
        path.touch()
        with self.assertRaises(ValueError):
            self.build()
        path.unlink()
        path.mkdir()
        with self.assertRaises(ValueError):
            self.build()
        path.rmdir()
        path.symlink_to(self.binaries / "metactl")
        with self.assertRaises(ValueError):
            self.build()
        self.assertEqual(before, archive.read_bytes())

    def test_real_cli_uses_tag_environment_and_rejects_mismatch(self):
        command = [sys.executable, str(Path(release.__file__)), "--target", release.TARGETS[0],
                   "--binary-dir", str(self.binaries), "--output-dir", str(self.root / "cli")]
        version = release.verify_version_consistency.package_version("crates/metactl/Cargo.toml")
        env = dict(os.environ, GITHUB_REF_TYPE="tag", GITHUB_REF_NAME="v999.0.0", SOURCE_DATE_EPOCH="1700000000")
        result = subprocess.run(command, env=env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match package version", result.stderr)
        self.assertFalse((self.root / "cli").exists())
        env["GITHUB_REF_NAME"] = "v" + version
        result = subprocess.run(command, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        env.update(GITHUB_REF_TYPE="branch", GITHUB_REF_NAME="main")
        result = subprocess.run(command, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
