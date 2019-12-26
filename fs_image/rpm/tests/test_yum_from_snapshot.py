#!/usr/bin/env python3
import json
import os
import tempfile
import subprocess
import unittest

from contextlib import contextmanager

from fs_image.common import load_location

from ..common import init_logging, Path
from ..yum_from_snapshot import yum_from_snapshot

_INSTALL_ARGS = ['install', '--assumeyes', 'rpm-test-carrot', 'rpm-test-milk']

init_logging()


class YumFromSnapshotTestCase(unittest.TestCase):

    @contextmanager
    def _yum_install(self, *, protected_paths, version_lock=None):
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
            snapshot_dir = Path(load_location('rpm', 'repo-snapshot'))
            # Note: this can't use `_yum_using_build_appliance` because that
            # would lose coverage info on `yum_from_snapshot.py`.  A
            # possible option is to try to make this test an
            # `image.python_unittest` that runs in the BA image, once
            # our BA image is guaranteed to have the versionlock plugin.
            # Right now, we use a host BA, which might not have it.
            with tempfile.NamedTemporaryFile(mode='w') as tf:
                if version_lock:
                    tf.write('\n'.join(version_lock) + '\n')
                tf.flush()
                yum_from_snapshot(
                    repo_server_bin=Path(load_location('rpm', 'repo-server')),
                    storage_cfg=json.dumps({
                        'key': 'test',
                        'kind': 'filesystem',
                        'base_dir': (snapshot_dir / 'storage').decode(),
                    }),
                    snapshot_dir=snapshot_dir,
                    install_root=Path(install_root),
                    protected_paths=protected_paths,
                    versionlock_list=tf.name,
                    yum_args=_INSTALL_ARGS,
                )
            yield install_root
        finally:
            assert install_root != '/'
            # Courtesy of `yum`, the `install_root` is now owned by root.
            subprocess.run(['sudo', 'rm', '-rf', install_root], check=True)

    def _check_installed_content(self, install_root, installed_content):
        # Remove known content so we can check there is nothing else.
        remove = []

        # Check that the RPMs installed their payload.
        for path, content in installed_content.items():
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

    def test_verify_contents_of_install_from_snapshot(self):
        milk = {
            'milk.txt': 'milk 2.71 8\n',
            'post.txt': 'stuff\n',  # From `milk-2.71` post-install
        }
        with self._yum_install(protected_paths=['meta/']) as install_root:
            self._check_installed_content(install_root, {
                **milk,
                'carrot.txt': 'carrot 2 rc0\n',
            })

        # Version-locking carrot causes a non-latest version to be installed
        with self._yum_install(
            protected_paths=['meta/'],
            version_lock=['0\trpm-test-carrot\t1\tlockme\tx86_64'],
        ) as install_root:
            self._check_installed_content(install_root, {
                **milk,
                'carrot.txt': 'carrot 1 lockme\n',
            })

        # Future: We'd actually want this to fail loudly instead of failing
        # to install the requested package, but this is what yum semantics
        # give us right now, and it'd take some effort to make it otherwise
        # (it's easier to do this error-checking in `RpmActionItem` anyway)
        with self._yum_install(
            protected_paths=['meta/'],
            version_lock=['0\trpm-test-carrot\t3333\tnonesuch\tx86_64'],
        ) as install_root:
            self._check_installed_content(install_root, milk)

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
