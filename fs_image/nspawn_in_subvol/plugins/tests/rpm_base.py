#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import tempfile
import textwrap

from fs_image.nspawn_in_subvol.tests.base import NspawnTestBase
from fs_image.rpm.find_snapshot import snapshot_install_dir


class RpmNspawnTestBase(NspawnTestBase):

    _SNAPSHOT_DIR = snapshot_install_dir(
        '//fs_image/rpm:repo-snapshot-for-tests'
    )

    def _yum_or_dnf_install(self, prog, package, *, extra_args=()):
        with tempfile.TemporaryFile(mode='w+b') as yum_dnf_stdout, \
                tempfile.TemporaryFile(mode='w+') as rpm_contents:
            # We don't pipe either stdout or stderr so that both are
            # viewable when running the test interactively.  We use `tee` to
            # obtain a copy of the program's stdout for tests.
            ret = self._nspawn_in((__package__, 'build-appliance'), [
                '--user=root',
                f'--serve-rpm-snapshot={self._SNAPSHOT_DIR}',
                f'--forward-fd={yum_dnf_stdout.fileno()}',  # becomes FD 3
                f'--forward-fd={rpm_contents.fileno()}',  # becomes FD 4
                *extra_args,
                '--',
                '/bin/sh', '-c',
                textwrap.dedent(f'''\
                    set -ex
                    mkdir /target
                    {prog} \\
                        --config={self._SNAPSHOT_DIR
                            }/{prog}/etc/{prog}/{prog}.conf \\
                        --installroot=/target -y install {package} |
                            tee /proc/self/fd/3
                    # We install only 1 RPM, so a glob tells us the filename.
                    # Use `head` instead of `cat` to fail nicer on exceeding 1.
                    head /target/rpm_test/*.txt >&4
                '''),
            ])
            # Hack up the `CompletedProcess` for ease of testing.
            yum_dnf_stdout.seek(0)
            ret.stdout = yum_dnf_stdout.read()
            rpm_contents.seek(0)
            ret.rpm_contents = rpm_contents.read()
            return ret

    def _check_yum_dnf_ret(self, expected_contents, expected_logline, ret):
        self.assertEqual(0, ret.returncode)
        self.assertEqual(expected_contents, ret.rpm_contents)
        self.assertIn(expected_logline, ret.stdout)
        self.assertIn(b'Complete!', ret.stdout)

    def _check_yum_dnf_boot_or_not(
        self, prog, package, *, extra_args=(), check_ret_fn=None,
    ):
        for boot_args in (['--boot'], []):
            check_ret_fn(self._yum_or_dnf_install(
                prog, package, extra_args=(*extra_args, *boot_args),
            ))
