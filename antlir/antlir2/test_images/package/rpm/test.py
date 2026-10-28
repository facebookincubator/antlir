#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest
from os import PathLike
from typing import Set


OUTPLACE_BASE = (
    "/usr/local/libexec/python_outplace/antlir_antlir2_test_images_package_rpm"
)
OUTPLACE_CONTAINER = f"{OUTPLACE_BASE}/outplace#outplace"


def _files_in_rpm(rpm: PathLike) -> Set[str]:
    return set(
        subprocess.run(
            ["rpm", "-qlp", rpm], check=True, capture_output=True, text=True
        ).stdout.splitlines()
    )


def _rpm_query(rpm: PathLike, *args: str) -> str:
    return subprocess.run(
        ["rpm", "-qp", *args, rpm], check=True, capture_output=True, text=True
    ).stdout


def _entries_in_rpm(rpm: PathLike) -> list[list[str]]:
    """`[mode, path, symlink target]` in header order, the order rpm installs in."""
    return [
        line.split("\t")
        for line in _rpm_query(
            rpm, "--qf", "[%{FILEMODES:perms}\t%{FILENAMES}\t%{FILELINKTOS}\n]"
        ).splitlines()
    ]


class RpmTest(unittest.TestCase):
    def test_stripped_binary(self):
        files = _files_in_rpm("/add.rpm")
        # The binary should be provided
        self.assertIn("/usr/bin/add", files)
        # It should also include debugging stuff, keyed by build-id
        self.assertIn("/usr/lib/.build-id", files)
        self.assertIn("/usr/lib/debug/.build-id", files)

    def test_no_ldconfig(self) -> None:
        files = _files_in_rpm("/add.rpm")
        self.assertIn("/usr/lib64/libadd.so.1.2", files)
        # but ldconfig should not have been run, so the symlink should not exist
        self.assertNotIn("/usr/lib64/libadd.so.1", files)

    def test_outplace_release_dir_and_current(self) -> None:
        entries = _entries_in_rpm("/outplace.rpm")
        paths = [path for _, path, _ in entries]
        links = {path: target for mode, path, target in entries if mode[0] == "l"}
        current = f"{OUTPLACE_CONTAINER}/~current"
        release = links[current]
        in_container = [p for p in paths if p.startswith(OUTPLACE_CONTAINER + "/")]
        self.assertEqual(
            {p[len(OUTPLACE_CONTAINER) + 1 :].split("/")[0] for p in in_container},
            {release, "~current"},
        )
        self.assertIn(f"{OUTPLACE_CONTAINER}/{release}/outplace-outplace#par", paths)
        self.assertIn(
            f"{OUTPLACE_CONTAINER}/{release}/outplace#link-tree/_bootstrap.sh", paths
        )
        # rpm repoints `~current` only after the whole release is in place,
        # and the launcher path only after `~current`
        compat = f"{OUTPLACE_BASE}/outplace-outplace#par"
        self.assertEqual(in_container[-1], current)
        self.assertEqual(
            links[compat], "outplace#outplace/~current/outplace-outplace#par"
        )
        self.assertGreater(paths.index(compat), paths.index(current))
        self.assertEqual(links["/usr/bin/outplace"], compat)

    def test_outplace_owns_its_dirs(self) -> None:
        entries = _entries_in_rpm("/outplace.rpm")
        dirs = {path for mode, path, _ in entries if mode[0] == "d"}
        files = [
            path
            for mode, path, _ in entries
            if mode[0] != "d" and path.startswith(OUTPLACE_CONTAINER + "/")
        ]
        self.assertTrue(files)
        # every dir between a file and the container is owned, so rpm removes
        # an old release dir once it is empty
        for f in files:
            d = os.path.dirname(f)
            while d.startswith(OUTPLACE_CONTAINER):
                self.assertIn(d, dirs)
                d = os.path.dirname(d)
        # nothing outside the container is claimed
        self.assertEqual(
            set(), {d for d in dirs if not d.startswith(OUTPLACE_CONTAINER)}
        )

    def test_outplace_disables_build_id_links(self) -> None:
        files = _files_in_rpm("/outplace.rpm")
        self.assertIn("/usr/bin/add", files)
        self.assertFalse(any(f.startswith("/usr/lib/.build-id") for f in files))
