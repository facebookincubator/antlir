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
        boot = os.getenv("BOOT")
        if boot == "False":
            self.assertNotEqual(
                0,
                subprocess.run(
                    ["systemctl", "is-active", "sysinit.target"],
                    check=False,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                ).returncode,
            )
        elif boot == "True":
            res = subprocess.run(
                ["systemctl", "is-active", "multi-user.target"],
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(
                res.returncode, 0, f"multi-user.target status: {res.stdout.strip()}"
            )
        elif boot == "wait-multi-user":
            res = subprocess.run(
                ["systemctl", "is-active", "multi-user.target"],
                capture_output=True,
                text=True,
            )
            self.assertEqual(
                res.returncode, 0, f"multi-user.target status: {res.stdout.strip()}"
            )
        else:
            self.fail(
                f"unrecognized boot mode '{boot}', update this test for any new behavior"
            )
