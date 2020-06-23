#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import tempfile
import subprocess
import unittest

from contextlib import contextmanager
from unittest import mock

from fs_image.fs_utils import create_ro, Path, temp_dir
from fs_image.common import init_logging
from fs_image.rpm.find_snapshot import snapshot_install_dir
from fs_image.rpm.yum_dnf_conf import YumDnf

from ..common import has_yum, yum_is_dnf
from .. import yum_dnf_from_snapshot


_INSTALL_ARGS = ['install', '--assumeyes', 'rpm-test-carrot', 'rpm-test-milk']
_SNAPSHOT_DIR = snapshot_install_dir('//fs_image/rpm:repo-snapshot-for-tests')

init_logging()


class YumFromSnapshotTestImpl:

    @contextmanager
    def _install(
        self, *, protected_paths, install_args=None
    ):
        if install_args is None:
            install_args = _INSTALL_ARGS
        install_root = Path(tempfile.mkdtemp())
        try:
            # IMAGE_ROOT/meta/ is always required since it's always protected
            for p in set(protected_paths) | {'meta/'}:
                if p.endswith('/'):
                    os.makedirs(install_root / p)
                else:
                    os.makedirs(os.path.dirname(install_root / p))
                    with open(install_root / p, 'wb'):
                        pass
            # Note: this can't use `_yum_using_build_appliance` because that
            # would lose coverage info on `yum_dnf_from_snapshot.py`.  On
            # the other hand, running this test against the host is fragile
            # since it depends on the system packages available on CI
            # containers.  For this reason, this entire test is an
            # `image.python_unittest` that runs in a build appliance.
            yum_dnf_from_snapshot.yum_dnf_from_snapshot(
                yum_dnf=self._YUM_DNF,
                snapshot_dir=_SNAPSHOT_DIR,
                protected_paths=protected_paths,
                yum_dnf_args=[
                    f'--installroot={install_root}',
                    *install_args,
                ]
            )
            yield install_root
        finally:
            assert install_root.realpath() != b'/'
            # Courtesy of `yum`, the `install_root` is now owned by root.
            subprocess.run(['sudo', 'rm', '-rf', install_root], check=True)

    def _check_installed_content(self, install_root, installed_content):
        # Remove known content so we can check there is nothing else.
        remove = []

        # Check that the RPMs installed their payload.
        for path, content in installed_content.items():
            remove.append(install_root / 'rpm_test' / path)
            with open(remove[-1]) as f:
                self.assertEqual(content, f.read())

        # Remove /bin/sh
        remove.append(install_root / 'bin/sh')

        prog_name = self._YUM_DNF.value

        # `yum` & `dnf` also write some indexes & metadata.
        for path in [
            f'var/lib/{prog_name}', 'var/lib/rpm', f'var/cache/{prog_name}',
            'usr/lib/.build-id'
        ]:
            remove.append(install_root / path)
            self.assertTrue(os.path.isdir(remove[-1]), remove[-1])
        remove.append(install_root / f'var/log/{prog_name}.log')
        self.assertTrue(os.path.exists(remove[-1]))
        if self._YUM_DNF == YumDnf.dnf:  # `dnf` loves log files
            for logfile in ['dnf.librepo.log', 'dnf.rpm.log', 'hawkey.log']:
                remove.append(install_root / 'var/log' / logfile)

        # Check that the above list of paths is complete.
        for path in remove:
            # We're running rm -rf as `root`, better be careful.
            self.assertTrue(path.startswith(install_root))
            # Most files are owned by root, so the sudo is needed.
            subprocess.run(['sudo', 'rm', '-rf', path], check=True)
        subprocess.run([
            'sudo', 'rmdir',
            'rpm_test', 'usr/lib', 'usr', 'var/lib', 'var/cache', 'var/log',
            'var/tmp', 'var', 'bin',
            *([
                'etc/dnf/modules.d', 'etc/dnf', 'etc'
            ] if self._YUM_DNF == YumDnf.dnf else []),
        ], check=True, cwd=install_root)
        required_dirs = {b'dev', b'meta'}
        self.assertEqual(required_dirs, set(install_root.listdir()))
        for d in required_dirs:
            self.assertEqual([], (install_root / d).listdir())

    def test_verify_contents_of_install_from_snapshot(self):
        milk = {
            'milk.txt': 'milk 2.71 8\n',
            'post.txt': 'stuff\n',  # From `milk-2.71` post-install
        }
        with self._install(protected_paths=['meta/']) as install_root:
            self._check_installed_content(install_root, {
                **milk,
                'carrot.txt': 'carrot 2 rc0\n',
            })

        # Fail when installing a package by its Provides: name, even when there
        # are more than one package in the list. Yum will only exit with an
        # error code here when specific options are explicitly set in the
        # yum.conf file.
        def _install_by_provides():
            return self._install(
                protected_paths=[],
                install_args=[
                    'install-n',
                    '--assumeyes',
                    'virtual-carrot',
                    'rpm-test-milk'
                ],
            )

        if self._YUM_DNF == YumDnf.yum:
            with self.assertRaises(subprocess.CalledProcessError):
                with _install_by_provides():
                    pass
        elif self._YUM_DNF == YumDnf.dnf:
            # DNF allows `install-n` to install by a "Provides:" name. We don't
            # particularly like the inconsistency with the behavior of yum, but
            # since we have a test for it, let's assert it here.
            with _install_by_provides() as install_root:
                self._check_installed_content(install_root, {
                    **milk,
                    'carrot.txt': 'carrot 2 rc0\n',
                })
        else:
            raise NotImplementedError(self._YUM_DNF)

    def test_fail_to_write_to_protected_path(self):
        # Nothing fails with no specified protection, or with /meta:
        for p in [[], ['meta/']]:
            with self._install(protected_paths=p):
                pass
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._install(protected_paths=['rpm_test/']):
                pass
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._install(protected_paths=['rpm_test/milk.txt']):
                pass
        # It was none other than `yum install` that failed.
        self.assertEqual(_INSTALL_ARGS, ctx.exception.cmd[-len(_INSTALL_ARGS):])

    def test_verify_install_to_container_root(self):
        # Hack alert: if we run both `{Dnf,Yum}FromSnapshotTestCase` in one
        # test invocation, the package manager that runs will just say that
        # the package is already install, and succeed.  That's OK.
        yum_dnf_from_snapshot.yum_dnf_from_snapshot(
            yum_dnf=self._YUM_DNF,
            snapshot_dir=_SNAPSHOT_DIR,
            protected_paths=[],
            yum_dnf_args=[
                # This is implicit: that also covers the "read the conf" code:
                # '--installroot=/',
                # `yum` fails without this since `/usr` is RO in the host BA.
                '--setopt=usr_w_check=false',
                'install-n', '--assumeyes', 'rpm-test-mice',
            ],
        )
        # Since we're running on /, asserting the effect on the complete
        # state of the filesystem would only be reasonable if we (a) took a
        # snapshot of the container "before", (b) took a snapshot of the
        # container "after", (c) rendered the incremental sendstream.  Since
        # incremental rendering is not implemented, settle for this basic
        # smoke-test for now.
        with open('/rpm_test/mice.txt') as infile:
            self.assertEqual('mice 0.1 a\n', infile.read())

    @contextmanager
    def _set_up_shadow(self, replacement, to_shadow):
        # Create the mountpoint at the shadowed location, and the file
        # that will shadow it.
        with create_ro(to_shadow, 'w'):
            pass
        with create_ro(replacement, 'w') as outfile:
            outfile.write('shadows carrot')

        # Shadow the file that `yum` / `dnf` wants to write -- writing to
        # this location will now fail since it's read-only.
        subprocess.check_call([
            'mount', '-o', 'bind,ro', replacement, to_shadow,
        ])
        try:
            yield
        finally:
            # Required so that our temporary dirs can be cleaned up.
            subprocess.check_call(['umount', to_shadow])

    def test_update_shadowed(self):
        with temp_dir() as root, mock.patch.object(
            # Note that the shadowed root is under the install root, since
            # the `rename` runs under chroot.
            yum_dnf_from_snapshot, 'SHADOWED_PATHS_ROOT', Path('/shadow'),
        ):
            os.mkdir(root / 'meta')
            os.mkdir(root / 'rpm_test')
            os.makedirs(root / 'shadow/rpm_test')

            to_shadow = root / 'rpm_test/carrot.txt'
            replacement = root / 'rpm_test/shadows_carrot.txt'
            shadowed_original = root / 'shadow/rpm_test/carrot.txt'

            # Our shadowing setup is supposed to have moved the original here.
            with create_ro(shadowed_original, 'w') as outfile:
                outfile.write('yum/dnf overwrites this')

            with self._set_up_shadow(replacement, to_shadow):
                with open(to_shadow) as infile:
                    self.assertEqual('shadows carrot', infile.read())
                with open(shadowed_original) as infile:
                    self.assertEqual('yum/dnf overwrites this', infile.read())

                yum_dnf_from_snapshot.yum_dnf_from_snapshot(
                    yum_dnf=self._YUM_DNF,
                    snapshot_dir=_SNAPSHOT_DIR,
                    protected_paths=[],
                    yum_dnf_args=[
                        f'--installroot={root}',
                        'install', '--assumeyes', 'rpm-test-carrot',
                    ],
                )

                # The shadow is still in place
                with open(to_shadow) as infile:
                    self.assertEqual('shadows carrot', infile.read())
                # But we updated the shadowed file
                with open(shadowed_original) as infile:
                    self.assertEqual('carrot 2 rc0\n', infile.read())


@unittest.skipIf(yum_is_dnf() or not has_yum(), "yum == dnf or yum missing")
class YumFromSnapshotTestCase(YumFromSnapshotTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.yum


class DnfFromSnapshotTestCase(YumFromSnapshotTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.dnf
