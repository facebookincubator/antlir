#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import getpass
import os
import subprocess
import unittest

from antlir.bzl.tests.coverage_test_helper import coverage_test_helper


class ImagePythonUnittestTest(unittest.TestCase):
    def test_container(self) -> None:
        # This should cause our 100% coverage assertion to pass.
        coverage_test_helper()
        self.assertEqual("nobody", getpass.getuser())
        # Test `internal_only_logs_tmpfs`: container /logs should be writable
        with open("/logs/garfield", "w") as catlog:
            catlog.write("Feed me.")
        # Future: add more assertions here as it becomes necessary what
        # aspects of test containers we actually care about.

    def test_env(self) -> None:
        # Ensure that per-test `env` settings do reach the container.
        self.assertEqual("meow", os.environ.pop("kitteh"))
        # Ensure that the container's environment is sanitized.
        env_allowlist = {
            # Antlir internals
            "ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP",
            "ANTLIR_BUCK",
            # Session basics
            "HOME",
            "LOGNAME",
            "NOTIFY_SOCKET",
            "PATH",
            "TERM",
            "USER",
            # Provided by the shell running the test
            "PWD",
            "SHLVL",
            # `nspawn --as-pid2` sets these 2, although they're quite silly.
            "container",
            "container_uuid",  # our nspawn runtime actually sets this to ''
            "container_host_id",  # systemd 246+
            "container_host_version_id",  # systemd 246+
            # These 2 are another `systemd` artifact, appearing when we pass
            # FDs into the container.
            "LISTEN_FDS",
            "LISTEN_PID",
            # PAR noise that doesn't start with `FB_PAR_` (filtered below)
            "PAR_LAUNCH_TIMESTAMP",
            "SCRIBE_LOG_USAGE",
            "LC_ALL",
            "LC_CTYPE",
            # FB test runner
            "TEST_PILOT",
            # FB "tcc coverage" mode seems to set these, mostly via
            # testinfra/testpilot/integration/python/adapters/coverage.py
            "COVERAGE_RCFILE",
            "PLATFORM",
            "PYTHON_SUBPROCESS_COVERAGE",
            "PY_IMPL",
            "PY_MAJOR",
            "PY_MINOR",
        }
        for var in os.environ:
            if var.startswith("FB_PAR_"):  # Set for non-in-place build modes
                continue
            self.assertIn(var, env_allowlist)
        # If the allowlist proves unmaintainable, Buck guarantees that this
        # variable is set, and it is NOT explicitly passed into containers,
        # so it ought to be absent.  See also `test-unsanitized-env`.
        self.assertNotIn("BUCK_BUILD_ID", os.environ)

    def test_layer_mount(self) -> None:
        # Verify that `/meownt` exists and is a mount point
        self.assertTrue(os.path.exists("/meownt"))
        subprocess.check_output(["/usr/bin/mountpoint", "-q", "/meownt"])

        # Verify that `/layer_mount` exists and is a mount point
        self.assertTrue(os.path.exists("/layer_mount"))
        subprocess.check_output(["/usr/bin/mountpoint", "-q", "/layer_mount"])

    def test_wrapped_runnable_args(self) -> None:
        self.assertTrue(os.path.exists("/foo/bar/installed/print-arg"))

        # Check arg with spaces to make sure the executable wrapping
        # works as expected
        self.assertEqual(
            subprocess.check_output(
                ["/foo/bar/installed/print-arg", "space cadet"], text=True
            ),
            "space cadet\n",
        )
