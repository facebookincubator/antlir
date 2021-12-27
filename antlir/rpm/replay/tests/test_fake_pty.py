# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest

from antlir.rpm.replay.fake_pty_wrapper import fake_pty_cmd, fake_pty_resource


class FakePtyTestCase(unittest.TestCase):
    def test_col_count(self):
        with fake_pty_resource() as fake_pty:
            col_count = (
                subprocess.run(
                    # Just run with whatever OS Python is available, the
                    # binary is intended to be polyglot.
                    [*fake_pty_cmd("/", fake_pty), "tput", "cols"],
                    check=True,
                    capture_output=True,
                    env={**os.environ, "TERM": "xterm"},
                )
                .stdout.decode()
                .strip()
            )
            self.assertEqual(col_count, "1000")
