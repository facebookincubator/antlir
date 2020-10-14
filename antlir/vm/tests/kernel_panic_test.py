#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import subprocess
import unittest
from pathlib import Path


class KernelPanicTest(unittest.TestCase):
    def test_vmtest_kernel_panic(self):
        resource = "antlir.vm.tests"
        with importlib.resources.path(resource, "vmtest") as vmtest:
            exe = Path(vmtest).resolve()

        proc = subprocess.run([exe], env={}, capture_output=True, text=True)
        combined = proc.stdout + proc.stderr

        self.assertIn("Kernel panic", combined)
        self.assertNotEqual(proc.returncode, 0)
