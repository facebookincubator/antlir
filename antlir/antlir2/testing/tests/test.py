#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest


class Test(unittest.TestCase):
    def test_is_root(self) -> None:
        self.assertEqual(0, os.getuid())

    def test_env_propagated(self) -> None:
        self.assertEqual("1", os.getenv("ANTLIR2_TEST"))

    def test_boot(self) -> None:
        if os.getenv("BOOT") != "False":
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
