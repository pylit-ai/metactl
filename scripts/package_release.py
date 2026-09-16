#!/usr/bin/env python3
"""Assemble release archives with explicit identity and normalized metadata."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile
import tempfile

import verify_version_consistency

ROOT = Path(__file__).resolve().parents[1]
TARGETS = ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu")
FILES = ("metactl", "metactld", "README.md", "LICENSE", "NOTICE")


def package(root: Path, binaries: Path, output: Path, version: str, target: str,
            epoch: int, tag: str | None = None) -> Path:
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("unsupported release version")
    if target not in TARGETS:
        raise ValueError("unsupported release target")
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"release tag {tag!r} does not match package version v{version}")
    if not 0 <= epoch <= 0o77777777777:
        raise ValueError("source epoch is outside the archive timestamp range")

    # Validate and snapshot every payload before creating or replacing artifacts.
    payloads = []
    for name in FILES:
        path = (binaries if name in FILES[:2] else root) / name
        if not stat.S_ISREG(path.lstat().st_mode):
            raise ValueError(f"release payload must be a regular file: {path}")
        data = path.read_bytes()
        if not data:
            raise ValueError(f"release payload is empty: {path}")
        payloads.append((name, data))

    prefix = f"metactl-v{version}-{target}"
    archive = output / f"{prefix}.tar.gz"
    output.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=output, delete=False) as raw:
            temporary = Path(raw.name)
            # Empty gzip filename and fixed gzip time remove host/path/time input.
            with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as tar:
                    directory = tarfile.TarInfo(prefix)
                    directory.type = tarfile.DIRTYPE
                    directory.mode = 0o755
                    directory.mtime = epoch
                    tar.addfile(directory)
                    for name, data in payloads:
                        info = tarfile.TarInfo(f"{prefix}/{name}")
                        info.size = len(data)
                        info.mode = 0o755 if name in FILES[:2] else 0o644
                        info.mtime = epoch
                        tar.addfile(info, io.BytesIO(data))
        temporary.replace(archive)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(archive.suffix + ".sha256").write_text(f"{digest}  {archive.name}\n")
    return archive


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--binary-dir", type=Path, default=ROOT / "target/release")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--tag", default=os.environ.get("GITHUB_REF_NAME")
                        if os.environ.get("GITHUB_REF_TYPE") == "tag" else None)
    args = parser.parse_args()
    verify_version_consistency.main()
    version = verify_version_consistency.package_version("crates/metactl/Cargo.toml")
    epoch = os.environ.get("SOURCE_DATE_EPOCH")
    if epoch is None:
        epoch = subprocess.check_output(["git", "log", "-1", "--format=%ct"], cwd=ROOT, text=True).strip()
    print(package(ROOT, args.binary_dir, args.output_dir, version, args.target, int(epoch), args.tag))


if __name__ == "__main__":
    main()
