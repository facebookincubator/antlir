# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import os
import unittest
from pathlib import Path


class TestPostscriptCgroup(unittest.TestCase):
    def test_not_left_behind(self) -> None:
        layer = Path(os.environ["ANTLIR2_POSTSCRIPT_CGROUP_SUBVOL_SYMLINK"]).resolve()
        debris = list((layer / "sys").glob("*"))
        self.assertFalse(debris, f"Debris found: {debris}")
