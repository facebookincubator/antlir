#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Ensure that the "non-build step" marking from `wrap_runtime_deps.bzl` is
correctly propagated to the VM, both to the user command, and to `systemd`
generators and units.

This is the analog of `test_boot_marked_as_non_build_step` in `test_run.py`.
"""
import os
import re
import subprocess
import time

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


class BuckRunnableVMTest(AntlirTestCase):
    def test_marked_as_non_build_step(self):
        while not re.search(
            "Process: .*code=exited",
            subprocess.run(
                ["systemctl", "status", "fake-static"],
                stdout=subprocess.PIPE,
                text=True,
            ).stdout,
        ):
            time.sleep(0.3)

        self.assertEqual(
            "1",
            os.environ.get("ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP"),
        )

        self.assertEqual(
            "fake_service: only_write_to_stdout\n",
            subprocess.check_output(
                ["/fake-service", "only_write_to_stdout"],
                text=True,
            ),
        )

        self.assertTrue(Path("/fake-static-service-ran").exists())
        # Regardless of "not a build step" marking, this
        # `install_buck_runnable` generator has no chance of running, in
        # MetalOS, the `rootfs` generators run after the switch-root away
        # from `initrd` but **before** the Antlir repo is mounted.  So when
        # the wrapper tries to execute `$REPO_ROOT/$(location ...)`, the
        # file will not be found.
        self.assertFalse(Path("/fake-systemd-generator-ran").exists())
