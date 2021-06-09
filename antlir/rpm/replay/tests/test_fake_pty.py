# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest

from antlir.fs_utils import Path


class FakePtyTestCase(unittest.TestCase):
    def test_col_count(self):
        with Path.resource(__package__, "fake_pty", exe=True) as fake_pty:
            col_count = (
                subprocess.run(
                    [fake_pty, "tput", "cols"],
                    check=True,
                    capture_output=True,
                    env={**os.environ, "TERM": "xterm"},
                )
                .stdout.decode()
                .strip()
            )
            self.assertEqual(col_count, "1000")
