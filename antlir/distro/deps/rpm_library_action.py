#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import glob
import json
import shutil
import subprocess
from pathlib import Path

INCLUDE_BASE = Path("/usr/include")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rpm-name", required=True)
    parser.add_argument("--lib", required=True)
    parser.add_argument("--header-glob", type=json.loads)
    parser.add_argument("--out-shared-lib", type=Path)
    parser.add_argument("--out-archive", type=Path)
    parser.add_argument("--out-headers", required=True, type=Path)
    args = parser.parse_args()

    headers = {}
    if args.header_glob:
        for subdir, pattern in args.header_glob:
            for p in glob.glob(pattern, root_dir=subdir, recursive=True):
                p = Path(p)
                dst = p.relative_to(subdir)
                headers[dst] = p
    else:
        rpm_files = {
            Path(p)
            for p in (
                subprocess.run(
                    ["rpm", "-q", "-l", args.rpm_name],
                    check=True,
                    text=True,
                    capture_output=True,
                )
                .stdout.strip()
                .splitlines()
            )
        }
        rpm_headers = {p for p in rpm_files if INCLUDE_BASE in p.parents}
        for h in rpm_headers:
            dst = h.relative_to(INCLUDE_BASE)
            headers[dst] = h

    for dst, src in headers.items():
        dst = args.out_headers / dst
        dst.parent.mkdir(parents=True, exist_ok=True)
        if src.is_dir():
            shutil.copytree(src, dst, dirs_exist_ok=True)
        else:
            shutil.copy2(src, dst)

    if args.out_shared_lib:
        if Path(args.lib).exists():
            libpath = Path(args.lib)
        else:
            libname = args.lib
            if not libname.startswith("lib"):
                libname = "lib" + libname
            libpath = Path("/usr/lib64") / libname
            if not libpath.exists():
                libpath = libpath.with_suffix(".so")

        shutil.copy2(libpath, args.out_shared_lib)

    if args.out_archive:
        if Path(args.lib).exists():
            libpath = Path(args.lib)
        else:
            libname = args.lib
            if not libname.startswith("lib"):
                libname = "lib" + libname
            libpath = Path("/usr/lib64") / libname
            if not libpath.exists():
                libpath = libpath.with_suffix(".a")
        shutil.copy2(libpath, args.out_archive)


if __name__ == "__main__":
    main()
