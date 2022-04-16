#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This test runs Buck-built binaries that were installed into an image.

Note that the implementation of executables in @mode/dev is quite
dramatically different from that in @mode/opt, so remember to run both while
developing to avoid later surprises from CI.
"""
import subprocess
import unittest

from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.common import nspawn_version
from antlir.nspawn_in_subvol.nspawn import run_nspawn

from .layer_resource import layer_resource_subvol


class ExecuteInstalledTestCase(unittest.TestCase):
    def _nspawn_in(self, rsrc_name, cmd, **kwargs):
        nsenter_proc, _console_proc = run_nspawn(
            new_nspawn_opts(
                cmd=cmd,
                layer=layer_resource_subvol(__package__, rsrc_name),
                quiet=True,  # Easier to assert the output.
            ),
            PopenArgs(**kwargs),
        )
        return nsenter_proc

    def test_execute(self):
        for print_ok in [
            "/foo/bar/installed/print-ok",
            "/foo/bar/installed/print-ok-too",
        ]:
            ret = self._nspawn_in(
                "exe-layer",
                [
                    # Workaround: When the test is compiled with LLVM
                    # profiling, then `print-ok` would try to write to
                    # `/default.profraw`, which is not permitted to the test
                    # user `nobody`.  This would print errors to stderr and
                    # cause our assertion below to fail.
                    "env",
                    "LLVM_PROFILE_FILE=/tmp/default.profraw",
                    # Required since this test is not an `image.*_unittest`
                    "ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1",
                    print_ok,
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(0, ret.returncode, (ret.stdout, ret.stderr))
            if nspawn_version().major >= 244:
                self.assertEqual((b"ok\n", b""), (ret.stdout, ret.stderr))
            else:
                # versions < 244 did not properly respect --quiet
                self.assertEqual(b"ok\n", ret.stdout)
