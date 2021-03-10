#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import os
import subprocess
import sys
from typing import List, NamedTuple, Set

from antlir.fs_utils import Path
from elftools.elf.dynamic import DynamicSection
from elftools.elf.elffile import ELFFile
from elftools.elf.segments import InterpSegment


class ExtractorOpts(NamedTuple):
    src_dir: Path
    dest_dir: Path
    files: List[Path]
    search: Set[Path] = set()


def parse(path: Path, opts: ExtractorOpts) -> Set[str]:
    """
    Parse an elf file and return a tuple of: The interpretor and a list of
    all needed libraries
    """
    with path.open("rb") as f:
        _elf = ELFFile(f)
        deps = set()

        for segment in _elf.iter_segments():
            if isinstance(segment, InterpSegment):
                # This is the interpreter
                interp = Path(segment.get_interp_name())
                interpdir = interp.dirname()
                # Resolve to the symlinks for the common case that the
                # interpreter is in /lib64 which is actually a symlink to
                # /usr/lib64
                if interpdir.islink():
                    interpdir = interpdir.realpath()
                    interp = interpdir / interp.basename()
                if os.path.isabs(interp):
                    interp = interp[1:]
                interp = opts.src_dir / interp
                interpdir = opts.src_dir / interpdir[1:]

                # Add the interp directory as a search path
                opts.search.add(interpdir)
                # Add the interpreter as a dependency
                deps.add(interp)

        # Get the RPATH/RUNPATH before looking for so files
        for section in _elf.iter_sections():
            if not isinstance(section, DynamicSection):
                continue
            for tag in section.iter_tags():
                if hasattr(tag, "rpath"):
                    rpath = Path(tag.rpath)
                    if os.path.isabs(rpath):
                        rpath = rpath.relpath("/")
                    opts.search.add(opts.src_dir / rpath)

        for section in _elf.iter_sections():
            if isinstance(section, DynamicSection):
                for tag in section.iter_tags():
                    if hasattr(tag, "needed"):
                        dep_so_name = tag.needed

                        # Search through the known search paths for the so
                        for search in opts.search:
                            dep_path = search / dep_so_name
                            if os.path.exists(dep_path):
                                deps.add(dep_path)
                                deps.update(parse(dep_path, opts))

    return deps


def extract(opts: ExtractorOpts):
    # The set of files to extract from
    # the src dir
    to_extract = set()

    for binary in opts.files:
        path = opts.src_dir / binary[1:]
        to_extract.add(path)
        to_extract = to_extract.union(parse(path, opts))

    for extract in to_extract:
        target = Path(
            opts.dest_dir / os.path.relpath(extract, start=opts.src_dir)
        )
        subprocess.run(["mkdir", "-p", target.dirname().decode()], check=True)

        subprocess.run(
            [
                "cp",
                "--recursive",
                "--no-clobber",
                "--dereference",
                "--reflink=auto",
                "--sparse=auto",
                "--preserve=all",
                "--no-preserve=links",
                extract.decode(),
                target.decode(),
            ],
            check=True,
        )


def _parse_args(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "files",
        nargs="+",
        type=Path.from_argparse,
        help="One or more binaries to inspect and extract.",
    )
    parser.add_argument(
        "--src-dir",
        required=True,
        type=Path.from_argparse,
        help="The source directory from where to find binaries",
    )
    parser.add_argument(
        "--dest-dir",
        required=True,
        type=Path.from_argparse,
        help="The destination directory to deposit found binaries + deps",
    )

    return ExtractorOpts(**Path.parse_args(parser, argv).__dict__)


def main(argv):
    extract(opts=_parse_args(argv))


if __name__ == "__main__":
    main(sys.argv[1:])
