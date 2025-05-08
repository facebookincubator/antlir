# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import subprocess
from unittest import TestCase


class Test(TestCase):
    def setUp(self) -> None:
        self.os = os.environ["OS"]
        os_release = {}
        with open("/etc/os-release", "r") as f:
            for line in f.readlines():
                key, value = line.strip().split("=")
                os_release[key] = value.strip('"')
        os_release_os = os_release["ID"] + os_release["VERSION_ID"]
        self.assertEqual(self.os, os_release_os)
        self.binary = f"/test/main-for-{self.os}"
        super().setUp()

    def test_binary_runs(self) -> None:
        """
        The simplest possible test is to just check if the binary runs at all
        """
        subprocess.run([self.binary], check=True)

    def test_using_system_interpreter(self) -> None:
        """
        Test that the built binary uses the system python interpreter, not the
        one for the fbcode platform (or any other os-agnostic platform
        configured in the buck build)
        """
        res = subprocess.run([self.binary], check=True, capture_output=True, text=True)
        res = json.loads(res.stdout)
        self.assertEqual(res["python_interpreter"], "/usr/bin/python3")

    def test_pex_looks_standalone(self) -> None:
        """
        Test that the built "binary" (pex) looks standalone, i.e. it doesn't
        have any references to 'buck-out/'
        """
        with open(self.binary, "rb") as f:
            binary_contents = f.read()
        # sanity check that 'python3' can be found, otherwise something is
        # really wrong
        self.assertIn(
            b"python3",
            binary_contents,
            "'python3' not found in pex contents, something looks very wrong",
        )
        self.assertNotIn(
            b"buck-out",
            binary_contents,
            "'buck-out' found in pex contents, it might not be standalone",
        )
