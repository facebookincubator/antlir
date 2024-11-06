#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

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
