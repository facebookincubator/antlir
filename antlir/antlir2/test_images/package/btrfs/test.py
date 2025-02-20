# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import re
import subprocess
import unittest


class Test(unittest.TestCase):
    def test_label(self) -> None:
        info = subprocess.run(
            ["btrfs", "filesystem", "label", os.environ["SIMPLE"]],
            text=True,
            capture_output=True,
            check=True,
        )
        self.assertEqual(info.stdout.strip(), "mylabel")

    def test_seed(self) -> None:
        proc = subprocess.run(
            ["btrfs", "inspect-internal", "dump-super", os.environ["SEED"]],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("SEEDING", proc.stdout)

        proc = subprocess.run(
            ["btrfs", "inspect-internal", "dump-super", os.environ["SIMPLE"]],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertNotIn("SEEDING", proc.stdout)

    def test_free_space(self) -> None:
        proc = subprocess.run(
            ["btrfs", "filesystem", "show", os.environ["FREE_SPACE"], "--raw"],
            check=True,
            capture_output=True,
            text=True,
        )

        match = re.search(r"\bsize\s+(\d+)\s+used\s+(\d+)\b", proc.stdout)
        self.assertIsNotNone(match, f"'{proc.stdout}' did not match")
        size = int(match.group(1))
        used = int(match.group(2))
        # The actual free space (size-used) should be at least the amount of
        # space that was requested
        self.assertGreaterEqual(size - used, int(os.environ["FREE_MB"]) * 1024 * 1024)
