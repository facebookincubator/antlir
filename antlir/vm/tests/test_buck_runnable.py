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
        self.assertTrue(Path("/fake-static-service-ran").exists())
        # The rootfs-based generator has no chance of running, since it's
        # not in the `initrd`, so there's no test coverage for whether
        # generators get the environment magic -- and since they go in the
        # always-packaged `initrd`, this doesn't really matter.
        self.assertFalse(Path("/fake-systemd-generator-ran").exists())
