#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import glob
import os
import shutil
import subprocess
from pathlib import Path

INCLUDE_BASE = Path("/usr/include")


def reljoin(a: Path, b: Path) -> Path:
    if b.is_absolute():
        return a / (b.relative_to("/"))
    return a / b


def pairs(iterable):
    iterators = [iter(iterable)] * 2
    return zip(*iterators, strict=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--rpm-name", required=True)
    parser.add_argument("--lib", required=True)
    parser.add_argument("--header-glob", action="append")
    parser.add_argument("--out-shared-lib", type=Path)
    parser.add_argument("--out-archive", type=Path)
    parser.add_argument("--out-headers", required=True, type=Path)
    parser.add_argument("--out-L-dir", type=Path)

    args = parser.parse_args()

    headers = {}
    if args.header_glob:
        for subdir, pattern in pairs(args.header_glob):
            subdir = reljoin(args.root, Path(subdir))
            if not subdir.exists():
                continue
            old_cwd = os.getcwd()
            os.chdir(subdir)
            for relpath in glob.glob(pattern, recursive=True):
                relpath = Path(relpath)
                headers[relpath] = subdir / relpath
            os.chdir(old_cwd)

    else:
        try:
            try:
                rpm = subprocess.run(
                    [
                        "rpm",
                        "--root",
                        str(args.root.resolve()),
                        "-q",
                        "--whatprovides",
                        args.rpm_name,
                    ],
                    check=True,
                    text=True,
                    capture_output=True,
                ).stdout.strip()
            except subprocess.CalledProcessError:
                rpm = args.rpm_name
            res = subprocess.run(
                ["rpm", "--root", str(args.root.resolve()), "-q", "-l", rpm],
                check=True,
                text=True,
                capture_output=True,
            )
        except subprocess.CalledProcessError as e:
            raise RuntimeError(e.stderr + "\n" + e.stdout) from e
        rpm_files = {Path(p) for p in (res.stdout.strip().splitlines())}
        rpm_headers = {p for p in rpm_files if INCLUDE_BASE in p.parents}
        for h in rpm_headers:
            dst = h.relative_to(INCLUDE_BASE)
            headers[dst] = args.root / h.relative_to("/")

    args.out_headers.mkdir()

    for dst, src in headers.items():
        dst = args.out_headers / dst
        dst.parent.mkdir(parents=True, exist_ok=True)
        if src.is_dir():
            shutil.copytree(src, dst, dirs_exist_ok=True)
        else:
            shutil.copy2(src, dst)

    if args.out_shared_lib:
        libpath = reljoin(args.root, Path(args.lib))
        if not libpath.exists():
            libname = args.lib
            if not libname.startswith("lib"):
                libname = "lib" + libname
            libpath = args.root / Path("usr/lib64") / libname
            if not libpath.exists():
                libpath = libpath.with_suffix(".so")

        shutil.copy2(libpath, args.out_shared_lib)

    if args.out_archive:
        libpath = reljoin(args.root, Path(args.lib))
        if not libpath.exists():
            libname = args.lib
            if not libname.startswith("lib"):
                libname = "lib" + libname
            libpath = args.root / Path("usr/lib64") / libname
            if not libpath.exists():
                libpath = libpath.with_suffix(".a")
        shutil.copy2(libpath, args.out_archive)

    if args.out_L_dir:
        args.out_L_dir.mkdir()
        shutil.copy2(args.out_shared_lib, args.out_L_dir)


if __name__ == "__main__":
    main()
