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


def _files_in_rpm(rpm: PathLike) -> Set[str]:
    return set(
        subprocess.run(
            ["rpm", "-qlp", rpm], check=True, capture_output=True, text=True
        ).stdout.splitlines()
    )


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

    def test_outplace_owns_link_tree_dirs(self) -> None:
        entries = subprocess.run(
            [
                "rpm",
                "-qp",
                "--qf",
                "[%{FILEMODES:perms} %{FILENAMES}\n]",
                "/outplace.rpm",
            ],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.splitlines()
        dirs = {e.split(" ", 1)[1] for e in entries if e.startswith("d")}
        link_tree_files = [
            e.split(" ", 1)[1]
            for e in entries
            if not e.startswith("d") and "#link-tree/" in e
        ]
        self.assertTrue(link_tree_files)
        # every parent dir of a link-tree file, up to the link-tree root, is owned
        for f in link_tree_files:
            d = os.path.dirname(f)
            while "#link-tree" in d:
                self.assertIn(d, dirs)
                d = os.path.dirname(d)
        # nothing outside the link-tree is claimed
        self.assertEqual(set(), {d for d in dirs if "#link-tree" not in d})

    def test_outplace_disables_build_id_links(self) -> None:
        files = _files_in_rpm("/outplace.rpm")
        self.assertIn("/usr/bin/add", files)
        self.assertFalse(any(f.startswith("/usr/lib/.build-id") for f in files))
