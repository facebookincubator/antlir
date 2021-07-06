#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import os
import pathlib
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from typing import List, NamedTuple, Set, Optional, Iterator

from antlir.common import get_logger
from antlir.fs_utils import Path
from elftools.elf.dynamic import DynamicSection
from elftools.elf.elffile import ELFFile
from elftools.elf.segments import InterpSegment


LDSO_RE = re.compile(
    r"^\s*(?P<name>.+)\s+=>\s+(?P<path>.+)\s+\(0x[0-9a-f]+\)$", re.MULTILINE
)


logger = get_logger()


@dataclass(frozen=True)
class ExtractorOpts(object):
    src_dir: Path
    dst_dir: Path
    binaries: List[Path]


@dataclass(frozen=True)
class ExtractFile(object):
    # Absolute path (including --src-dir if applicable) where a file should be
    # copied from.
    src: Path
    # Absolute path (not including --dst-dir) where a file should be copied to.
    dst: Path


# Joining absolute paths is annoying, so make a helper function that makes it
# easy.
def force_join(*paths: Path) -> Path:
    if not paths:
        return Path(b"/")
    path = paths[0]
    for component in paths[1:]:
        if os.path.isabs(component):
            path = path / component.relpath("/")
        else:
            path = path / component
    return path


def find_interpreter(binary: Path) -> Optional[Path]:
    with binary.open("rb") as f:
        elf = ELFFile(f)
        for segment in elf.iter_segments():
            if isinstance(segment, InterpSegment):
                return Path(segment.get_interp_name())
    return None


# In all the cases that we care about, a library will live under /lib64, but
# this directory will be a symlink to /usr/lib64. To avoid build conflicts with
# other image layers, replace it.
def ensure_usr(path: Path) -> Path:
    if path.startswith(b"/usr"):
        return path
    else:
        return force_join(Path("/usr"), path)


@dataclass
class Binary(object):
    # root to consider for this binary, either / or --src-dir
    root: Path
    file: ExtractFile
    interpreter: Path

    def __init__(self, root: Path, src: Path, dst: Path):
        self.root = root
        self.file = ExtractFile(src, dst)
        interp = find_interpreter(src)
        if interp is None:
            interp = Path("/usr/lib64/ld-linux-x86-64.so.2")
            logger.warn(
                f"no interpreter found for {src}, falling back to '{interp}'"
            )
        self.interpreter = interp

    # Find all transitive dependencies of this binary. Return ExtractFile
    # objects for this binary, its interpreter and all dependencies.
    def extracts(self) -> Iterator[ExtractFile]:
        yield self.file
        ldso_out = subprocess.run(
            [
                force_join(self.root, self.interpreter),
                "--list",
                self.file.src,
            ],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        for match in LDSO_RE.finditer(ldso_out):
            path = Path(match.group("path"))
            # There is not a bulletproof way to tell if a dependency is
            # supposed to be relative to the source location or not based
            # solely on the ld.so output.
            # As a simple heuristic, guess that if the directory is the
            # same as that of the binary, it should be installed at the
            # same relative location to the binary destination.
            # Importantly, this heuristic can only ever produce an
            # incorrect result with buck-built binaries (the only kind
            # where destination is not necessarily the same as the source),
            # and if a binary really has an absolute dependency on buck-out,
            # there is nothing we can do about it.
            bin_src_parent = self.file.src.dirname()
            if path.startswith(bin_src_parent):
                relpath = path.relpath(bin_src_parent)
                bin_dst_parent = self.file.dst.dirname()
                yield ExtractFile(
                    src=force_join(self.root, path),
                    dst=bin_dst_parent / relpath,
                )
            else:
                yield ExtractFile(
                    src=force_join(self.root, path), dst=ensure_usr(path)
                )
        yield ExtractFile(
            src=force_join(self.root, self.interpreter),
            dst=ensure_usr(self.interpreter),
        )


def _parse_args(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary",
        dest="binaries",
        action="append",
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
        "--dst-dir",
        required=True,
        type=Path.from_argparse,
        help="The destination directory to deposit found binaries + deps",
    )

    return ExtractorOpts(**Path.parse_args(parser, argv).__dict__)


def extract(opts: ExtractorOpts):
    copy_files = {}
    for path in opts.binaries:
        binary = Binary(opts.src_dir, path, path)
        copy_files.update({e.dst: e.src for e in binary.extracts()})
    for dst, src in copy_files.items():
        real_dst = force_join(opts.dst_dir, dst)
        os.makedirs(real_dst.dirname(), exist_ok=True)
        shutil.copy(src, str(real_dst))

    # do a bottom-up traversal of all the destination directories, copying the
    # permissions bits from the source directory where possible
    copied_dirs = set()
    for dst in copy_files.keys():
        for parent in pathlib.Path(str(dst)).parents:
            copied_dirs.add(Path(parent))

    copied_dirs = sorted(
        copied_dirs,
        key=lambda d: len(pathlib.Path(str(d)).parents),
        reverse=True,
    )
    for rel_dst_dir in copied_dirs:
        dst_dir = force_join(opts.dst_dir, rel_dst_dir)
        maybe_src_dir = force_join(opts.src_dir, rel_dst_dir)
        # Only copy mode bits form directories that match the source dir.
        # This prevents incorrect modes when a single file is copied into a
        # directory from a directory with "bad" permissions
        if maybe_src_dir.exists():
            shutil.copystat(str(maybe_src_dir), str(dst_dir))
            st = os.stat(maybe_src_dir)
            os.chown(dst_dir, uid=st.st_uid, gid=st.st_gid)


def main(argv):
    extract(opts=_parse_args(argv))


if __name__ == "__main__":
    main(sys.argv[1:])
