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

    def test_compiler_version(self) -> None:
        """
        Test the compiler version used to build the binary, to make sure that it
        looks like it came from the right target-os toolchain.
        """
        out = json.loads(
            subprocess.run(
                [self.binary], check=True, capture_output=True, text=True
            ).stdout
        )
        clang_version = out["clang_version"]
        self.assertTrue(
            clang_version.endswith(f".el{self.os_version_id})"),
            f"clang {clang_version!r} doesn't look like it came from the right toolchain",
        )

    def test_linked_version_matches_installed(self) -> None:
        """
        Check that the demo binary is actually linked against the system version
        of 'rpm' by making sure its output matches the version of 'rpm' that is
        actually installed.
        """
        installed_version = subprocess.run(
            ["rpm", "-q", "--queryformat", "%{V}", "rpm-libs"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        out = json.loads(
            subprocess.run(
                [self.binary], check=True, capture_output=True, text=True
            ).stdout
        )
        self.assertEqual(installed_version, out["rpmlib_version"])

    def test_rpm_dependencies(self) -> None:
        """
        Ensure that rpmbuild automatically finds the system dependencies that
        are linked against. That is the way to safely deploy a system-linked
        binary and letting rpmbuild do it is great to avoid user mistakes
        forgetting to define dependencies.
        """
        requires = set(
            subprocess.run(
                ["rpm", "-q", "--requires", "main"],
                check=True,
                capture_output=True,
                text=True,
            )
            .stdout.strip()
            .splitlines()
        )
        self.assertTrue(
            any(r.startswith("librpm.so") for r in requires),
            "'main' did not require librpm.so",
        )

    def test_platform_preprocessor_flags(self) -> None:
        """
        Check that the preprocessor flags are set based on platform regex
        matches in cxx rules.
        """
        out = json.loads(
            subprocess.run(
                [self.binary], check=True, capture_output=True, text=True
            ).stdout
        )
        platform_preprocessor_flag = out["platform_preprocessor_flag"]
        self.assertEqual(
            platform_preprocessor_flag,
            f"{self.os}-{platform.machine()}",
        )
