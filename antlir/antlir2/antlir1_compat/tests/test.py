#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_is_root(self) -> None:
        self.assertEqual(0, os.getuid())

    def test_booted(self) -> None:
        if os.getenv("BOOT") == "1":
            subprocess.run(
                ["systemctl", "is-active", "-q", "sysinit.target"],
                check=True,
            )
        else:
            self.assertNotEqual(
                0,
                subprocess.run(
                    ["systemctl", "is-active", "sysinit.target"],
                    check=False,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                ).returncode,
            )
