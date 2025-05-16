# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import platform
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
        self.os_version_id = os_release["VERSION_ID"]
        super().setUp()

    def test_binary_runs(self) -> None:
        """
        The simplest possible test is to just check if the binary runs at all
        """
        subprocess.run([self.binary], check=True)

    def test_using_system_interpreter(self) -> None:
        """
        Test that the built binary uses the system dynamic linker, not the one
        for the fbcode platform (or any other os-agnostic platform configured in
        the buck build)
        """
        stdout = subprocess.run(
            ["readelf", "-l", self.binary], check=True, capture_output=True, text=True
        ).stdout
        for line in stdout.splitlines():
            line = line.strip()
            if not line.startswith("[Requesting program interpreter: "):
                continue
            interp = line.removeprefix(
                "[Requesting program interpreter: "
            ).removesuffix("]")
            self.assertEqual(
                interp,
                {
                    "x86_64": "/lib64/ld-linux-x86-64.so.2",
                    "aarch64": "/lib/ld-linux-aarch64.so.1",
                }[platform.machine()],
            )

    def test_build_mode(self) -> None:
        """
        Test that the built binary's recorded BUILD_MODE matches the test env var
        """
        out = json.loads(
            subprocess.run(
                [self.binary], check=True, capture_output=True, text=True
            ).stdout
        )
        self.assertEqual(os.environ["BUILD_MODE"], out["build_mode"])
