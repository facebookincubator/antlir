#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import subprocess
import tempfile
import unittest

from antlir.fs_utils import Path


class KernelPanicTest(unittest.TestCase):
    def test_vmtest_kernel_panic(self):
        with importlib.resources.path(
            __package__, "create-kernel-panic"
        ) as vmtest, tempfile.NamedTemporaryFile() as console_f:

            # This is the running the fully materialized =vmtest script
            # that buck built.
            proc = subprocess.run(
                [Path(vmtest), "--append-console={}".format(console_f.name)],
            )

            # Expect to see the kernel panic message in the console output
            self.assertIn(
                b"Kernel panic - not syncing: sysrq triggered crash",
                console_f.read(),
            )
