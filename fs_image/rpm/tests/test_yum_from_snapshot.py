#!/usr/bin/env python3
import os
import tempfile
import subprocess
import unittest

from contextlib import contextmanager

from ..common import init_logging, Path

from .yum_from_test_snapshot import yum_from_test_snapshot

_INSTALL_ARGS = ['install', '--assumeyes', 'rpm-test-carrot', 'rpm-test-milk']

init_logging()


class YumFromSnapshotTestCase(unittest.TestCase):

    @contextmanager
    def _yum_install(self, *, protected_paths):
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

            yum_from_test_snapshot(
                install_root,
                protected_paths=protected_paths,
                yum_args=_INSTALL_ARGS,
            )
            yield install_root
        finally:
            assert install_root != '/'
            # Courtesy of `yum`, the `install_root` is now owned by root.
            subprocess.run(['sudo', 'rm', '-rf', install_root], check=True)

    def test_verify_contents_of_install_from_snapshot(self):
        with self._yum_install(protected_paths=['meta/']) as install_root:
            # Remove known content so we can check there is nothing else.
            remove = []

            # Check that the RPMs installed their payload.
            for path, content in [
                ('milk.txt', 'milk 2.71 8\n'),
                ('carrot.txt', 'carrot 2 rc0\n'),
                ('post.txt', 'stuff\n'),
            ]:
                remove.append(install_root / 'usr/share/rpm_test' / path)
                with open(remove[-1]) as f:
                    self.assertEqual(content, f.read())

            # Remove /bin/sh
            remove.append(install_root / 'bin/sh')

            # Yum also writes some indexes & metadata.
            for path in [
                    'var/lib/yum', 'var/lib/rpm', 'var/cache/yum',
                    'usr/lib/.build-id'
            ]:
                remove.append(install_root / path)
                self.assertTrue(os.path.isdir(remove[-1]))
            remove.append(install_root / 'var/log/yum.log')
            self.assertTrue(os.path.exists(remove[-1]))

            # Check that the above list of paths is complete.
            for path in remove:
                # We're running rm -rf as `root`, better be careful.
                self.assertTrue(path.startswith(install_root))
                # Most files are owned by root, so the sudo is needed.
                subprocess.run(['sudo', 'rm', '-rf', path], check=True)
            subprocess.run([
                'sudo', 'rmdir',
                'usr/share/rpm_test', 'usr/share', 'usr/lib', 'usr',
                'var/lib', 'var/cache', 'var/log', 'var/tmp', 'var',
                'bin',
            ], check=True, cwd=install_root)
            required_dirs = sorted([b'dev', b'meta'])
            self.assertEqual(required_dirs, sorted(os.listdir(install_root)))
            for d in required_dirs:
                self.assertEqual([], os.listdir(install_root / d))

    def test_fail_to_write_to_protected_path(self):
        # Nothing fails with no specified protection, or with /meta:
        for p in [[], ['meta/']]:
            with self._yum_install(protected_paths=p):
                pass
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._yum_install(protected_paths=['usr/share/rpm_test/']):
                pass
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._yum_install(protected_paths=[
                'usr/share/rpm_test/milk.txt'
            ]):
                pass
        # It was none other than `yum install` that failed.
        self.assertEqual(_INSTALL_ARGS, ctx.exception.cmd[-len(_INSTALL_ARGS):])
