#!/usr/bin/env python3
"""Reject the known private feature in Git trees and public release archives."""

import argparse
import io
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import zipfile


PRIVATE_PATH = re.compile(
    rb"(^|[/\\])(thurea(-guard)?[-_ ]pass(\.[a-z]+)?|porn[-_ ]?pass(\.[a-z]+)?|"
    rb"pass(-state)?\.json|chromium-policy-pass\.json)([/\\]|$)", re.IGNORECASE
)
ARCHIVE_SUFFIXES = (".zip", ".crx", ".apk", ".tar.zst", ".deb", ".rpm", ".tar.xz", ".tar.gz")
PRIVATE_CODE = (
    r"""pbPass|PB_PASS|PASS_URL_RE|porn[:_ ]pass|["']porn-pass["']|pass\.json|"""
    r"Thurea( Guard)? pass[[:space:]:]|chromium-policy-pass|id=.tabAdult"
)


def check_tree(commit):
    names = subprocess.check_output(["git", "ls-tree", "-rz", "--name-only", commit])
    for name in names.decode(errors="surrogateescape").split("\0"):
        if PRIVATE_PATH.search(name.encode(errors="surrogateescape")):
            raise ValueError(f"{commit}: {name}")
        if name.lower().endswith(ARCHIVE_SUFFIXES):
            data = subprocess.check_output(["git", "show", f"{commit}:{name}"])
            check_artifact(name, data)
    result = subprocess.run(
        ["git", "grep", "-iIlE", PRIVATE_CODE, commit, "--", ".", ":(exclude)release/check-public.py"],
        stdout=subprocess.PIPE,
    )
    if result.returncode == 0:
        raise ValueError(result.stdout.decode(errors="replace").strip())
    if result.returncode != 1:
        raise RuntimeError("git grep failed")


def check_artifact(name, data):
    if PRIVATE_PATH.search(name.encode(errors="surrogateescape")):
        raise ValueError(name)
    if name.lower().endswith((".zip", ".crx", ".apk")):
        # Python's ZIP reader also handles the CRX header prepended to Chromium extensions.
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            for member in archive.infolist():
                if not member.is_dir():
                    check_artifact(f"{name}/{member.filename}", archive.read(member))
    elif name.lower().endswith((".tar.zst", ".deb", ".rpm", ".tar.xz", ".tar.gz", ".tar")):
        if name.lower().endswith(".tar.zst"):
            raw = subprocess.run(["zstd", "-dc"], input=data, stdout=subprocess.PIPE, check=True).stdout
        elif name.lower().endswith((".deb", ".rpm")):
            # libarchive reads both (a .deb's members are its control and data tarballs).
            raw = subprocess.run(["bsdtar", "-cf", "-", "--format", "pax", "@-"], input=data, stdout=subprocess.PIPE, check=True).stdout
        else:
            raw = data
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:*") as archive:
            for member in archive:
                path = f"{name}/{member.name}"
                if PRIVATE_PATH.search(path.encode(errors="surrogateescape")):
                    raise ValueError(path)
                if member.isfile():
                    check_artifact(path, archive.extractfile(member).read())
    else:
        marker = PRIVATE_CODE.replace("[[:space:]:]", r"[\s:]")
        if re.search(marker.encode(), data, re.IGNORECASE):
            raise ValueError(name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument("--commits", nargs="+")
    inputs.add_argument("--assets", nargs="+", type=Path)
    args = parser.parse_args()
    try:
        if args.commits:
            for commit in args.commits:
                check_tree(commit)
        else:
            for asset in args.assets:
                check_artifact(asset.name, asset.read_bytes())
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError,
            zipfile.BadZipFile, tarfile.TarError) as error:
        print(f"Thurea publication blocked by the private-feature check: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
