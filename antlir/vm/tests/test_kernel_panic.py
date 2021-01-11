#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import subprocess
import unittest

from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.common import DEFAULT_PATH_ENV


class KernelPanicTest(unittest.TestCase):
    def test_vmtest_kernel_panic(self):
        with importlib.resources.path(__package__, "vmtest") as vmtest:
            exe = Path(vmtest)

        proc = subprocess.run(
            [exe],
            env={"PATH": DEFAULT_PATH_ENV},
            capture_output=True,
            text=True,
        )

        combined = f"\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"

        # Expect vmtest failed with QemuError
        self.assertEqual(proc.returncode, 255, proc.returncode)
        self.assertTrue(
            "Communication with VM failed: " in combined, msg=combined
        )
