# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import os
import subprocess
import unittest


class TestIncremental(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()
        subprocess.run(["mkfs.btrfs", "/dev/vdb"], check=True)
        os.mkdir("/mnt/recv")
        subprocess.run(["mount", "/dev/vdb", "/mnt/recv"], check=True)

    def test_cannot_receive_child_as_is(self) -> None:
        """
        It's very important to test that receiving the child FAILS before
        receiving the parent, otherwise we could fool ourselves into believing
        that this works but in fact are relying on the parent subvolume still
        existing and being usable somewhere on the build host.
        """
        with importlib.resources.path(__package__, "child.sendstream") as child:
            proc = subprocess.run(
                ["btrfs", "receive", "-m", "/mnt/recv", "/mnt/recv", "-f", child],
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(proc.returncode, 0, "btrfs-receive should have failed")

    def test_receive(self) -> None:
        with importlib.resources.path(__package__, "parent.sendstream") as parent:
            subprocess.run(
                ["btrfs", "receive", "-m", "/mnt/recv", "/mnt/recv", "-f", parent],
                text=True,
                capture_output=True,
                check=True,
            )
        os.mkdir("/mnt/recv/child")
        with importlib.resources.path(__package__, "child.sendstream") as child:
            subprocess.run(
                ["btrfs", "receive", "-m", "/mnt/recv", "/mnt/recv/child", "-f", child],
                text=True,
                capture_output=True,
            )

        self.assertTrue(os.path.exists("/mnt/recv/child/volume/parent_large_file"))
        self.assertTrue(os.path.exists("/mnt/recv/child/volume/child_large_file"))
