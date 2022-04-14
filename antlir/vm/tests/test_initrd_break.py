#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import subprocess
import tempfile

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


class InitrdBreakTest(AntlirTestCase):
    def test_vmtest_initrd_break_default(self):
        with importlib.resources.path(
            __package__, "vmtest-initrd-break-default"
        ) as vmtest, tempfile.NamedTemporaryFile() as console_f:

            # Run the buck built vmtest target instance.
            subprocess.run(
                [
                    Path(vmtest),
                    "--append-console={}".format(console_f.name),
                ],
            )

            # Check for the expected `systectl list-jobs` output.
            console_output = str(console_f.read())
            for i in [
                r"debug-shell\.service +start running",
                r"initrd\.target +start waiting",
            ]:
                self.assertRegex(console_output, i)

    def test_vmtest_initrd_break_custom(self):
        with importlib.resources.path(
            __package__, "vmtest-initrd-break-custom"
        ) as vmtest, tempfile.NamedTemporaryFile() as console_f:

            # Run the buck built vmtest target instance.
            subprocess.run(
                [
                    Path(vmtest),
                    "--append-console={}".format(console_f.name),
                ],
            )

            # Check for the expected `systectl list-jobs` output.
            console_output = str(console_f.read())
            for i in [
                r"debug-shell\.service +start running",
                r"sysinit\.target +start waiting",
            ]:
                self.assertRegex(console_output, i)
