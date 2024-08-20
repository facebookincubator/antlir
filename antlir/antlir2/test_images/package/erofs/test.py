# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import subprocess
import unittest


class Test(unittest.TestCase):
    def test_metadata(self) -> None:
        with importlib.resources.path(__package__, "simple.erofs") as path:
            meta = subprocess.run(
                ["dump.erofs", path, "--path=/test"],
                check=True,
                text=True,
                capture_output=True,
            ).stdout
            self.assertIn("directory", meta)
            self.assertIn("Uid: 0", meta)
            self.assertIn("Gid: 0", meta)
            self.assertIn("Access: 0700", meta)

            meta = subprocess.run(
                ["dump.erofs", path, "--path=/test/file"],
                check=True,
                text=True,
                capture_output=True,
            ).stdout
            self.assertIn("regular file", meta)
            self.assertIn("Uid: 0", meta)
            self.assertIn("Gid: 0", meta)
            self.assertIn("Access: 0000", meta)
