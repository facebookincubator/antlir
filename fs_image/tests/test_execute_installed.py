#!/usr/bin/env python3
'''
This test runs Buck-built binaries that were installed into an image.

Note that the implementation of executables in @mode/dev is quite
dramatically different from that in @mode/opt, so remember to run both while
developing to avoid later surprises from CI.
'''
import os
import subprocess
import unittest

from find_built_subvol import find_built_subvol
from fs_image.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from fs_image.nspawn_in_subvol.common import _nspawn_version
from fs_image.nspawn_in_subvol.non_booted import run_non_booted_nspawn


class ExecuteInstalledTestCase(unittest.TestCase):

    def _nspawn_in(self, rsrc_name, cmd, **kwargs):
        return run_non_booted_nspawn(new_nspawn_opts(
            cmd=cmd,
            # __file__ works in @mode/opt since the resource is inside the XAR
            layer=find_built_subvol(
                os.path.join(os.path.dirname(__file__), rsrc_name)
            ),
            quiet=True,  # Easier to assert the output.
        ), PopenArgs(**kwargs))

    def test_execute(self):
        for print_ok in [
            '/foo/bar/installed/print-ok',
            '/foo/bar/installed/print-ok-too',
        ]:
            ret = self._nspawn_in(
                'exe-layer', [
                    # Workaround: When the test is compiled with LLVM
                    # profiling, then `print-ok` would try to write to
                    # `/default.profraw`, which is not permitted to the test
                    # user `nobody`.  This would print errors to stderr and
                    # cause our assertion below to fail.
                    'env', 'LLVM_PROFILE_FILE=/tmp/default.profraw',
                    print_ok,
                ],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            if _nspawn_version() >= 244:
                self.assertEqual((b'ok\n', b''), (ret.stdout, ret.stderr))
            else:
                # versions < 244 did not properly respect --quiet
                self.assertEqual(b'ok\n', ret.stdout)
