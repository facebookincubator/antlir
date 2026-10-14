#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Generate an unsigned apt archive index (dists/<dist>/Release and
dists/<dist>/main/binary-<arch>/Packages) for a set of local .deb files.
"""

import argparse
import gzip
import hashlib
import io
import lzma
import os
import tarfile

_COMPONENT = "main"

_DECOMPRESSORS = {
    "control.tar.xz": lzma.decompress,
    "control.tar.gz": gzip.decompress,
    "control.tar": lambda b: b,
}


def _ar_members(data: bytes) -> dict[str, bytes]:
    if not data.startswith(b"!<arch>\n"):
        raise ValueError("not an ar archive")
    members = {}
    offset = 8
    while offset < len(data):
        header = data[offset : offset + 60]
        name = header[:16].decode().strip().rstrip("/")
        size = int(header[48:58].decode().strip())
        offset += 60
        members[name] = data[offset : offset + size]
        # ar members are 2-byte aligned
        offset += size + (size % 2)
    return members


def _control(deb: bytes) -> str:
    members = _ar_members(deb)
    for name, decompress in _DECOMPRESSORS.items():
        if name not in members:
            continue
        with tarfile.open(fileobj=io.BytesIO(decompress(members[name]))) as tar:
            for member in tar.getmembers():
                if os.path.normpath(member.name) == "control":
                    f = tar.extractfile(member)
                    assert f is not None
                    return f.read().decode().strip("\n")
        raise ValueError(f"{name} has no control file")
    raise ValueError(f"no supported control archive in {sorted(members)}")


def _field(control: str, name: str) -> str:
    for line in control.splitlines():
        key, _, value = line.partition(":")
        if key == name:
            return value.strip()
    raise ValueError(f"control file has no {name} field:\n{control}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--distribution", required=True)
    parser.add_argument("--arch", action="append", default=[])
    parser.add_argument(
        "--deb",
        action="append",
        default=[],
        help="FILENAME=PATH where FILENAME is the path relative to the archive root",
    )
    args = parser.parse_args()

    stanzas = []
    for spec in args.deb:
        filename, path = spec.split("=", 1)
        with open(path, "rb") as f:
            data = f.read()
        control = _control(data)
        stanza = (
            f"{control}\n"
            f"Filename: {filename}\n"
            f"Size: {len(data)}\n"
            f"SHA256: {hashlib.sha256(data).hexdigest()}\n"
        )
        stanzas.append((_field(control, "Architecture"), stanza))

    dist_dir = os.path.join(args.out, "dists", args.distribution)
    checksums = []
    for arch in args.arch:
        relpath = f"{_COMPONENT}/binary-{arch}/Packages"
        packages = "\n".join(s for a, s in stanzas if a in (arch, "all")).encode()
        os.makedirs(os.path.dirname(os.path.join(dist_dir, relpath)), exist_ok=True)
        with open(os.path.join(dist_dir, relpath), "wb") as f:
            f.write(packages)
        checksums.append(
            f" {hashlib.sha256(packages).hexdigest()} {len(packages)} {relpath}"
        )

    # A fixed Date in the past keeps the output reproducible without apt
    # rejecting the Release as "not valid yet"
    release = "\n".join(
        [
            "Origin: antlir2",
            f"Suite: {args.distribution}",
            f"Codename: {args.distribution}",
            "Date: Thu, 01 Jan 1970 00:00:00 UTC",
            f"Architectures: {' '.join(args.arch)}",
            f"Components: {_COMPONENT}",
            "SHA256:",
            *checksums,
        ]
    )
    with open(os.path.join(dist_dir, "Release"), "w") as f:
        f.write(release + "\n")


if __name__ == "__main__":
    main()
