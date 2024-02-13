# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import os
import subprocess
import unittest


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()
        subprocess.run(["mkfs.btrfs", "/dev/vdb"], check=True)
        os.mkdir("/mnt/recv")
        subprocess.run(["mount", "/dev/vdb", "/mnt/recv"], check=True)

    def _test(self, sendstream, volume="volume") -> None:
        subprocess.run(
            ["btrfs", "receive", "-m", "/mnt/recv", "/mnt/recv", "-f", sendstream],
            check=True,
        )
        self.assertEqual(os.listdir("/mnt/recv"), [volume])
        os.chdir(f"/mnt/recv/{volume}")
        with open("foo/bar/hello") as f:
            self.assertEqual(f.read(), "Hello world\n")
        hello = os.stat("foo/bar/hello")
        self.assertEqual(hello.st_uid, 42)
        self.assertEqual(hello.st_gid, 43)

    def test_sendstream(self) -> None:
        with importlib.resources.path(__package__, "layer.sendstream") as s:
            self._test(s)

    def test_sendstream_v2(self) -> None:
        with importlib.resources.path(__package__, "layer.sendstream.v2") as s:
            self._test(s)

    def test_named_sendstream_v2(self) -> None:
        with importlib.resources.path(__package__, "named.sendstream.v2") as s:
            self._test(s, volume="named")
