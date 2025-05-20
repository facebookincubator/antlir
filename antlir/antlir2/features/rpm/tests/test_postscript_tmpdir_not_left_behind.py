# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import os
import subprocess
import unittest


class TestPostscriptTmpdirNotLeftBehind(unittest.TestCase):
    def test_not_left_behind(self) -> None:
        subprocess.run(["umount", "/tmp"], check=True)
        self.assertFalse(list(os.listdir("/tmp")))
