# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import re
import subprocess
import unittest


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()
        self.maxDiff = None
        subprocess.run(["mount", "-o", "subvolid=5", "/dev/vdb", "/mnt"], check=True)

    def test_default_subvol(self) -> None:
        info = subprocess.run(
            ["btrfs", "subvolume", "get-default", "/mnt"],
            text=True,
            capture_output=True,
            check=True,
        )
        self.assertIn("foo/baz/qux", info.stdout.strip())

    def test_ro_rw(self) -> None:
        info = subprocess.run(
            ["btrfs", "property", "get", "/mnt/foo"],
            text=True,
            capture_output=True,
            check=True,
        )
        self.assertEqual(info.stdout.strip(), "ro=true")

        info = subprocess.run(
            ["btrfs", "property", "get", "/mnt/bar"],
            text=True,
            capture_output=True,
            check=True,
        )
        self.assertEqual(info.stdout.strip(), "ro=false")

    def test_label(self) -> None:
        info = subprocess.run(
            ["btrfs", "filesystem", "label", "/mnt"],
            text=True,
            capture_output=True,
            check=True,
        )
        self.assertEqual(info.stdout.strip(), "mylabel")

    def test_contents(self) -> None:
        with open("/mnt/foo/foo") as f:
            self.assertEqual(f.read(), "foo")
        with open("/mnt/bar/bar") as f:
            self.assertEqual(f.read(), "bar")
        with open("/mnt/foo/baz/qux/qux") as f:
            self.assertEqual(f.read(), "qux")

    def test_seed(self) -> None:
        proc = subprocess.run(
            ["btrfs", "inspect-internal", "dump-super", "/dev/vdc"],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("SEEDING", proc.stdout)

        proc = subprocess.run(
            ["btrfs", "inspect-internal", "dump-super", "/dev/vdb"],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertNotIn("SEEDING", proc.stdout)

    def test_free_space(self) -> None:
        proc = subprocess.run(
            ["btrfs", "filesystem", "show", "/dev/vdd", "--raw"],
            check=True,
            capture_output=True,
            text=True,
        )

        match = re.search(r"\bsize\s+(\d+)\s+used\s+(\d+)\b", proc.stdout)
        self.assertIsNotNone(match, f"'{proc.stdout}' did not match")
        size = int(match.group(1))
        used = int(match.group(2))
        self.assertAlmostEqual(
            size - used, int(os.environ["FREE_MB"]) * 1024 * 1024, delta=1024 * 1024
        )
