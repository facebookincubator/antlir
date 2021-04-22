# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest


class TestRust(unittest.TestCase):
    def test_rustc_version(self):
        version = subprocess.run(
            ["rustc", "--version"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

        self.assertIn("nightly", version)
