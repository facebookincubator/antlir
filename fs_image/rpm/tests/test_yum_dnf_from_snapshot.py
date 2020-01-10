#!/usr/bin/env python3
import json
import os
import tempfile
import subprocess
import unittest

from contextlib import contextmanager

from ..common import init_logging, Path
from ..yum_dnf_from_snapshot import YumDnf, yum_dnf_from_snapshot

_INSTALL_ARGS = ['install', '--assumeyes', 'rpm-test-carrot', 'rpm-test-milk']

init_logging()


class YumFromSnapshotTestImpl:

    @contextmanager
    def _install(self, *, protected_paths, version_lock=None):
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
            # This is the same hard coded path used by the `RpmActionItem` in
            # the compiler.
            snapshot_dir = Path("/rpm-repo-snapshot/default")
            # Note: this can't use `_yum_using_build_appliance` because that
            # would lose coverage info on `yum_dnf_from_snapshot.py`.  On
            # the other hand, running this test against the host is fragile
            # since it depends on the system packages available on CI
            # containers.  For this reason, this entire test is an
            # `image.python_unittest` that runs in a build appliance.
            with tempfile.NamedTemporaryFile(mode='w') as tf:
                if version_lock:
                    tf.write('\n'.join(version_lock) + '\n')
                tf.flush()
                yum_dnf_from_snapshot(
                    yum_dnf=self._YUM_DNF,
                    repo_server_bin=Path(snapshot_dir) / 'repo-server',
                    storage_cfg=json.dumps({
                        'key': 'test',
                        'kind': 'filesystem',
                        'base_dir': (snapshot_dir / 'storage').decode(),
                    }),
                    snapshot_dir=snapshot_dir,
                    install_root=Path(install_root),
                    protected_paths=protected_paths,
                    versionlock_list=tf.name,
                    yum_dnf_args=_INSTALL_ARGS,
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
            'usr/share/rpm_test', 'usr/share', 'usr/lib', 'usr',
            'var/lib', 'var/cache', 'var/log', 'var/tmp', 'var',
            'bin', *([
                'etc/dnf/modules.d', 'etc/dnf', 'etc'
            ] if self._YUM_DNF == YumDnf.dnf else []),
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
        with self._install(protected_paths=['meta/']) as install_root:
            self._check_installed_content(install_root, {
                **milk,
                'carrot.txt': 'carrot 2 rc0\n',
            })

        # Version-locking carrot causes a non-latest version to be installed
        with self._install(
            protected_paths=['meta/'],
            version_lock=['0\trpm-test-carrot\t1\tlockme\tx86_64'],
        ) as install_root:
            self._check_installed_content(install_root, {
                **milk,
                'carrot.txt': 'carrot 1 lockme\n',
            })

        def _install_nonexistent():
            return self._install(
                protected_paths=['meta/'],
                version_lock=['0\trpm-test-carrot\t3333\tnonesuch\tx86_64'],
            )

        if self._YUM_DNF == YumDnf.yum:
            # For `yum`, we'd actually want this to fail loudly instead of
            # failing to install the requested package, but this is what it
            # does now, and it'd take some effort to make it otherwise (it's
            # easier to do this error-checking in `RpmActionItem` anyway)
            with _install_nonexistent() as install_root:
                self._check_installed_content(install_root, milk)
        elif self._YUM_DNF == YumDnf.dnf:
            # Unlike `yum`, `dnf` actually fails with:
            #   Error: Unable to find a match: rpm-test-carrot
            with self.assertRaises(subprocess.CalledProcessError):
                with _install_nonexistent():
                    pass
        else:
            raise NotImplementedError(self._YUM_DNF)

    def test_fail_to_write_to_protected_path(self):
        # Nothing fails with no specified protection, or with /meta:
        for p in [[], ['meta/']]:
            with self._install(protected_paths=p):
                pass
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._install(protected_paths=['usr/share/rpm_test/']):
                pass
        with self.assertRaises(subprocess.CalledProcessError) as ctx:
            with self._install(protected_paths=[
                'usr/share/rpm_test/milk.txt'
            ]):
                pass
        # It was none other than `yum install` that failed.
        self.assertEqual(_INSTALL_ARGS, ctx.exception.cmd[-len(_INSTALL_ARGS):])


class YumFromSnapshotTestCase(YumFromSnapshotTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.yum


class DnfFromSnapshotTestCase(YumFromSnapshotTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.dnf
