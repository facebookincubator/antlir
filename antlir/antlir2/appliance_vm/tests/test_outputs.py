# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest


class TestUname(unittest.TestCase):
    def test_vm_uname(self) -> None:
        with open(os.environ["UNAME"]) as f:
            vm_uname = f.read().strip()

        self.assertEqual(vm_uname, "6.4.3")

    def test_alt_rootfs(self) -> None:
        with open(os.environ["ALT_ROOTFS"]) as f:
            text = f.read().strip()

        self.assertEqual(text, "bar")
