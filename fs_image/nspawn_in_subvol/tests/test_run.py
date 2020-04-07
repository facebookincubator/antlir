#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools
import os
import subprocess
import sys
import tempfile
import textwrap
import threading
import unittest

from contextlib import contextmanager
from pwd import struct_passwd
from typing import Iterable
from unittest import mock

from fs_image.artifacts_dir import find_repo_root
from fs_image.common import pipe
from fs_image.rpm.find_snapshot import DEFAULT_SNAPSHOT_INSTALL_DIR
from fs_image.tests.temp_subvolumes import with_temp_subvols

from ..args import _parse_cli_args, PopenArgs
from ..common import _nspawn_version
from ..cmd import _extra_nspawn_args_and_env
from ..run import _set_up_run_cli


@contextmanager
def _mocks_for_parse_cli_args():
    with mock.patch(
        'fs_image.nspawn_in_subvol.args.pwd.getpwnam'
    ) as getpwnam_mock, mock.patch(
        'fs_image.nspawn_in_subvol.args.find_built_subvol'
    ) as find_built_subvol_mock:
        getpwnam_mock.side_effect = [struct_passwd([
            'pw_name', 'pw_passwd', 123, 123, 'pw_gecos', '/test/home',
            '/test/sh',
        ])]
        find_built_subvol_mock.side_effect = [None]
        yield


@contextmanager
def _mocks_for_extra_nspawn_args(*, artifacts_may_require_repo):
    with mock.patch(
        'fs_image.nspawn_in_subvol.cmd._artifacts_may_require_repo'
    ) as amrr_mock:
        amrr_mock.side_effect = [artifacts_may_require_repo]
        yield


class NspawnTestCase(unittest.TestCase):
    def setUp(self):
        # Setup expected stdout line endings depending on the version of
        # systemd-nspawn.  Version 242 'fixed' stdout line endings.  The
        # extra newline for versions < 242 is due to T40936918 mentioned in
        # `run.py`.  It would disappear if we passed `--quiet` to nspawn,
        # but we want to retain the extra debug logging.
        self.nspawn_version = _nspawn_version()
        self.maybe_extra_ending = b'\n' if self.nspawn_version < 242 else b''

    def _nspawn_in_boot_ret(self, rsrc_name, argv, **kwargs):
        # It'd be nice to replace this `__file__` with `Path.resource`, but
        # the challenge is that `rsrc_name` is a directory, which does
        # not work in the current model. Possible fixes are:
        #   - Make `Path.resource` handle directories transparently. This
        #     is clean, but for large directories is grossly inefficient.
        #   - Add some nasty hackery to resolve the `layer.json` that we
        #     actually need from this directory, as a resource.
        layer_path = os.path.join(os.path.dirname(__file__), rsrc_name)
        with _set_up_run_cli(['--layer', layer_path, *argv]) as cli_setup:
            if 'boot_console' in kwargs:
                cli_setup = cli_setup._replace(
                    boot_console=kwargs.pop('boot_console')
                )
            return cli_setup._run_nspawn(PopenArgs(**kwargs))

    def _nspawn_in(self, rsrc_name, argv, **kwargs):
        ret, _boot_ret = self._nspawn_in_boot_ret(rsrc_name, argv, **kwargs)
        return ret

    def _wrapper_args_to_nspawn_args(
        self, argv, *, artifacts_may_require_repo=False,
    ):
        with _mocks_for_parse_cli_args():
            args = _parse_cli_args(argv, allow_debug_only_opts=True)
        with _mocks_for_extra_nspawn_args(
            artifacts_may_require_repo=artifacts_may_require_repo,
        ):
            args, _env = _extra_nspawn_args_and_env(args.opts)
            return args

    def _assertIsSubseq(self, subseq, seq, msg=None):
        subseqs = [
            seq[i:i + len(subseq)] for i in range(len(seq) - len(subseq) + 1)
        ]
        self.assertIn(subseq, subseqs)

    def test_extra_nspawn_args_private_network_opts(self):
        # opts.private_network
        self.assertIn('--private-network', self._wrapper_args_to_nspawn_args(
            ['--layer', 'test']
        ))
        # !opts.private_network
        self.assertNotIn('--private-network', self._wrapper_args_to_nspawn_args(
            ['--layer', 'test', '--no-private-network']
        ))

    def test_extra_nspawn_args_bindmount_opts(self):
        # opts.bindmount_rw
        self._assertIsSubseq(['--bind', '/src:/dest'],
            self._wrapper_args_to_nspawn_args(
                ['--layer', 'test', '--bindmount-rw', '/src', '/dest']
            )
        )
        # opts.bindmount_ro
        self._assertIsSubseq(['--bind-ro', '/src:/dest'],
            self._wrapper_args_to_nspawn_args(
                ['--layer', 'test', '--bindmount-ro', '/src', '/dest']
            )
        )

    @mock.patch('fs_image.nspawn_in_subvol.cmd.find_repo_root')
    def test_extra_nspawn_args_bind_repo_opts(self, root_mock):
            root_mock.return_value = '/repo/root'
            # opts.bind_repo_ro
            self.assertIn('/repo/root:/repo/root',
                self._wrapper_args_to_nspawn_args(
                    ['--layer', 'test', '--bind-repo-ro']
                )
            )
            # artifacts_may_require_repo
            self.assertIn('/repo/root:/repo/root',
                self._wrapper_args_to_nspawn_args(
                    ['--layer', 'test'], artifacts_may_require_repo=True,
                )
            )

    def test_extra_nspawn_args_log_tmpfs_opts(self):
        base_argv = ['--layer', 'test', '--user=is_mocked']
        # opts.logs_tmpfs
        self.assertIn(
            '--tmpfs=/logs:uid=123,gid=123,mode=0755,nodev,nosuid,noexec',
            self._wrapper_args_to_nspawn_args(base_argv),
        )
        # !opts.logs_tmpfs
        self.assertNotIn(
            '--tmpfs=/logs:uid=123,gid=123,mode=0755,nodev,nosuid,noexec',
            self._wrapper_args_to_nspawn_args(base_argv + ['--no-logs-tmpfs']),
        )

    def test_extra_nspawn_args_cap_net_admin_opts(self):
        # opts.cap_net_admin
        self.assertIn(
            '--capability=CAP_NET_ADMIN', self._wrapper_args_to_nspawn_args(
                ['--layer', 'test', '--cap-net-admin']
            ),
        )

    def test_extra_nspawn_args_hostname_opts(self):
        # opts.hostname
        self.assertIn(
            '--hostname=test-host', self._wrapper_args_to_nspawn_args(
                ['--layer', 'test', '--hostname', 'test-host']
            ),
        )

    def test_extra_nspawn_args_quiet_opts(self):
        # opts.quiet
        self.assertIn('--quiet', self._wrapper_args_to_nspawn_args(
            ['--layer', 'test', '--quiet']
        ))

    def test_extra_nspawn_args_foward_tls_env_opts(self):
        # opts.foward_tls_env
        with _mocks_for_parse_cli_args():
            args = _parse_cli_args(
                ['--layer', 'test', '--forward-tls-env'],
                allow_debug_only_opts=True,
            )
        with _mocks_for_extra_nspawn_args(
            artifacts_may_require_repo=False,
        ), mock.patch.dict(os.environ, {'THRIFT_TLS_TEST': 'test_val'}):
            _nspawn_args, cmd_env = _extra_nspawn_args_and_env(args.opts)
            self.assertIn('THRIFT_TLS_TEST=test_val', cmd_env)

    def test_nspawn_version(self):
        with mock.patch('subprocess.check_output') as version:
            version.return_value = (
                'systemd 602214076 (v602214076-2.fb1)\n+AVOGADROS SYSTEMD\n')
            self.assertEqual(602214076, _nspawn_version())

        # Check that the real nspawn on the machine running this test is
        # actually a sane version.  We need at least 239 to do anything useful
        # and 1000 seems like a reasonable upper bound, but mostly I'm just
        # guessing here.
        self.assertTrue(_nspawn_version() > 239)
        self.assertTrue(_nspawn_version() < 1000)

    def test_exit_code(self):
        self.assertEqual(37, self._nspawn_in(
            'host', ['--', 'sh', '-c', 'exit 37'], check=False,
        ).returncode)

    def test_redirects(self):
        cmd = ['--', 'sh', '-c', 'echo ohai && echo abracadabra >&2']
        ret = self._nspawn_in(
            'host', cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(b'ohai\n' + self.maybe_extra_ending, ret.stdout)

        # stderr is not just a clean `abracadabra\n` because we don't
        # suppress nspawn's debugging output, hence the 'assertIn'.
        self.assertIn(b'abracadabra\n', ret.stderr)

        # The same test with `--quiet` is much simpler.
        ret = self._nspawn_in(
            'host', ['--quiet'] + cmd,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(b'ohai\n', ret.stdout)
        target_stderr = b'abracadabra\n'
        if _nspawn_version() >= 244:
            self.assertEqual(target_stderr, ret.stderr)
        else:
            # versions < 244 did not properly respect --quiet
            self.assertIn(target_stderr, ret.stderr)

    def test_machine_id(self):
        # Images meant to be used as a container root should include an empty
        # /etc/machine-id file. This file will be populated by the system
        # manager when it first boots the container. Furthermore, this will
        # not cause the firstboot + presets behavior that is triggered when the
        # machine-id file does not exist.
        self._nspawn_in('bootable-systemd-os', [
            '--', 'sh', '-uexc',
            # Ensure the machine-id file exists and is empty.
            'test -e /etc/machine-id -a ! -s /etc/machine-id',
        ])

    def test_logs_directory(self):
        # The log directory is on by default.
        ret = self._nspawn_in('host', [
            '--', 'sh', '-c',
            'touch /logs/foo && stat --format="%U %G %a" /logs && whoami',
        ], stdout=subprocess.PIPE)
        self.assertEqual(0, ret.returncode)
        self.assertEqual(
            b'nobody nobody 755\nnobody\n' + self.maybe_extra_ending,
            ret.stdout
        )
        # And the option prevents it from being created.
        self.assertEqual(0, self._nspawn_in('host', [
            '--no-logs-tmpfs', '--', 'test', '!', '-e', '/logs',
        ]).returncode)

    def test_forward_fd(self):
        with tempfile.TemporaryFile() as tf:
            tf.write(b'hello')
            tf.seek(0)
            ret = self._nspawn_in('host', [
                '--forward-fd', str(tf.fileno()), '--', 'sh', '-c',
                'cat <&3 && echo goodbye >&3',
            ], stdout=subprocess.PIPE)
            self.assertEqual(b'hello' + self.maybe_extra_ending, ret.stdout)
            tf.seek(0)
            self.assertEqual(b'hellogoodbye\n', tf.read())

    @with_temp_subvols
    def test_non_ephemeral_snapshot(self, temp_subvols):
        dest_subvol = temp_subvols.caller_will_create('persistent')
        # We won't create this subvol by manipulating this very object, but
        # rather indirectly through its path.  So its _exists would never
        # get updated, which would cause the TempSubvolumes cleanup to fail.
        # Arguably, the cleanup should be robust to this, but since this is
        # the unique place we have to do it, keep it simple.
        dest_subvol._exists = True
        self._nspawn_in('host', [
            f'--snapshot-into={dest_subvol.path()}', '--',
            # Also tests that we are a non-root user in the container.
            'sh', '-c', 'echo ohaibai "$USER" > /home/nobody/poke',
        ])
        with open(dest_subvol.path('/home/nobody/poke')) as f:
            self.assertEqual('ohaibai nobody\n', f.read())
        # Spot-check: the host mounts should still be available on the snapshot
        self.assertTrue(os.path.exists(dest_subvol.path('/bin/bash')))

    def test_bind_repo(self):
        self._nspawn_in('host', [
            '--bind-repo-ro', '--',
            'grep', 'supercalifragilisticexpialidocious',
            os.path.join(
                os.path.realpath(find_repo_root(sys.argv[0])),
                'fs_image/nspawn_in_subvol/tests',
                os.path.basename(__file__),
            ),
        ])

    def test_cap_net_admin(self):
        self._nspawn_in('host', [
            '--user', 'root', '--no-private-network', '--cap-net-admin', '--',
            'unshare', '--net', 'ip', 'link', 'set', 'dev', 'lo', 'up',
        ])

    def test_hostname(self):
        ret = self._nspawn_in('host', [
            '--hostname=test-host.com',
            '--',
            '/bin/hostname',
        ], stdout=subprocess.PIPE, check=True)
        self.assertEqual(b'test-host.com\n', ret.stdout)

    @mock.patch.dict('os.environ', {
        'THRIFT_TLS_KITTEH': 'meow', 'UNENCRYPTED_KITTEH': 'woof',
    })
    def test_tls_environment(self):
        ret = self._nspawn_in('host', [
            '--forward-tls-env', '--',
            'printenv', 'THRIFT_TLS_KITTEH', 'UNENCRYPTED_KITTEH',
        ], stdout=subprocess.PIPE, check=False)
        self.assertNotEqual(0, ret.returncode)  # UNENCRYPTED_KITTEH is unset
        self.assertEqual(b'meow\n' + self.maybe_extra_ending, ret.stdout)

    def test_bindmount_rw(self):
        with tempfile.TemporaryDirectory() as tmpdir, \
                tempfile.TemporaryDirectory() as tmpdir2:
            self._nspawn_in('host', [
                '--user',
                'root',
                '--bindmount-rw',
                tmpdir, '/tmp',
                '--bindmount-rw',
                tmpdir2, '/mnt',
                '--',
                'touch',
                '/tmp/testfile',
                '/mnt/testfile',
            ])
            self.assertTrue(os.path.isfile(f'{tmpdir}/testfile'))
            self.assertTrue(os.path.isfile(f'{tmpdir2}/testfile'))

    def test_bindmount_ro(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with self.assertRaises(subprocess.CalledProcessError):
                ret = self._nspawn_in('host', [
                    '--user',
                    'root',
                    '--bindmount-ro',
                    tmpdir, '/tmp',
                    '--',
                    'touch',
                    '/tmp/testfile',
                ])
                self.assertEqual(
                    "touch: cannot touch '/tmp/testfile': " +
                        'Read-only file system',
                    ret.stdout,
                )

    def test_xar(self):
        'Make sure that XAR binaries work in vanilla `buck run` containers'
        ret = self._nspawn_in('host-hello-xar', [
            '--', '/hello.xar',
        ], stdout=subprocess.PIPE, check=True)
        self.assertEqual(b'hello world\n' + self.maybe_extra_ending, ret.stdout)

    def test_mknod(self):
        'CAP_MKNOD is dropped by our runtime.'
        ret = self._nspawn_in('host', [
            '--user', 'root', '--quiet', '--', 'mknod', '/foo', 'c', '1', '3',
        ], stderr=subprocess.PIPE, check=False)
        self.assertNotEqual(0, ret.returncode)
        target_stderr = b"mknod: '/foo': Operation not permitted\n"
        if _nspawn_version() >= 244:
            self.assertEqual(target_stderr, ret.stderr)
        else:
            # versions < 244 did not properly respect --quiet
            self.assertIn(target_stderr, ret.stderr)

    def test_boot_cmd_is_system_running(self):
        ret = self._nspawn_in('bootable-systemd-os', [
            '--boot',
            # This needs to be root because we don't yet create a proper
            # login session for non-privileged users when we execute commands.
            # Systemctl will try and connect to the user session
            # when it's run as non-root.
            '--user=root',
            '--',
            '/usr/bin/systemctl', 'is-system-running', '--wait',
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)

        # The assertions here are validating that the systemd instance inside
        # the container completed its boot but *not* that it successfully
        # started every service.  This reason for this is that it's not
        # a gauranteed property of the image build system that the image
        # successfully boot systemd, but simply that systemd can be properly
        # started and 'complete' a boot.  The success of the boot is really
        # something that can only be properly evaulated by unit tests for
        # a specific image.
        self.assertIn(ret.returncode, [0, 1], msg=ret.stderr.strip())
        self.assertIn(ret.stdout.strip(),
                [b'running', b'degraded'], msg=ret.stderr.strip())
        # versions < 244 did not properly respect --quiet
        if _nspawn_version() >= 244:
            self.assertEqual(b'', ret.stderr)

    def test_boot_cmd_failure(self):
        ret = self._nspawn_in('bootable-systemd-os', [
            '--boot',
            '--',
            '/usr/bin/false',
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        self.assertEqual(1, ret.returncode)
        self.assertEqual(b'', ret.stdout)
        # versions < 244 did not properly respect --quiet
        if _nspawn_version() >= 244:
            self.assertEqual(b'', ret.stderr)

    def test_boot_forward_fd(self):
        with tempfile.TemporaryFile() as tf:
            tf.write(b'hello')
            tf.seek(0)
            ret = self._nspawn_in('bootable-systemd-os', [
                '--boot',
                '--forward-fd', str(tf.fileno()),
                '--',
                '/usr/bin/sh',
                '-c',
                '/usr/bin/cat <&3 && /usr/bin/echo goodbye >&3',
            ], stdout=subprocess.PIPE, check=True)
            self.assertEqual(b'hello', ret.stdout)
            tf.seek(0)
            self.assertEqual(b'hellogoodbye\n', tf.read())

    def test_boot_unprivileged_user(self):
        ret = self._nspawn_in('bootable-systemd-os', [
            '--boot',
            '--',
            '/bin/whoami',
        ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
        self.assertEqual(0, ret.returncode)
        self.assertEqual(b'nobody\n', ret.stdout)
        self.assertEqual(b'', ret.stderr)

    def test_boot_env_clean(self):
        ret = self._nspawn_in('bootable-systemd-os', [
            '--boot',
            '--',
            '/bin/env',
        ], stdout=subprocess.PIPE, check=True)
        self.assertEqual(0, ret.returncode)

        # Verify we aren't getting anything in from the outside we don't want
        self.assertNotIn(b'BUCK_BUILD_ID', ret.stdout)

        # Verify we get what we expect
        self.assertIn(b'HOME', ret.stdout)
        self.assertIn(b'PATH', ret.stdout)
        self.assertIn(b'LOGNAME', ret.stdout)
        self.assertIn(b'USER', ret.stdout)
        self.assertIn(b'TERM', ret.stdout)

    def test_boot_proc_results(self):
        console_singleton = []
        with pipe() as (r, w):

            def read_console():
                nonlocal console_singleton
                console_singleton.append(r.read())

            # We have to consume the read end of the pipe from a thread
            # because otherwise `systemd` could deadlock during shutdown.
            # This is explained in the `booted.py` docblock.
            reader_thread = threading.Thread(target=read_console)
            reader_thread.start()

            ret, ret_boot = self._nspawn_in_boot_ret('bootable-systemd-os', [
                '--boot',
                '--',
                '/bin/true',
            ], boot_console=w, check=True)
        reader_thread.join()

        self.assertEqual(0, ret.returncode)

        self.assertIsNotNone(ret_boot)
        self.assertIsNotNone(ret_boot.returncode)
        self.assertEqual(0, ret_boot.returncode)

        console, = console_singleton
        self.assertIn(b'Welcome to', console)
        self.assertIn(b'Reached target', console)
        self.assertIn(b'Stopped target', console)

    def test_boot_error(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self._nspawn_in(
                'bootable-systemd-os',
                ['--boot', '--', '/bin/false'],
            )

    def test_boot_no_console_pipe(self):
        with self.assertRaisesRegex(
            RuntimeError, ' does not support `subprocess.PIPE` ',
        ):
            # This is identical to `test_boot_proc_results` except for the pipe
            self._nspawn_in('bootable-systemd-os', [
                '--boot',
                '--',
                '/bin/true',
            ], boot_console=subprocess.PIPE, check=True)

    def _yum_or_dnf_install(self, prog, package, *, extra_args=()):
        snapshot_dir = DEFAULT_SNAPSHOT_INSTALL_DIR
        with tempfile.TemporaryFile(mode='w+b') as yum_dnf_stdout, \
                tempfile.TemporaryFile(mode='w+') as rpm_contents:
            # We don't pipe either stdout or stderr so that both are
            # viewable when running the test interactively.  We use `tee` to
            # obtain a copy of the program's stdout for tests.
            ret = self._nspawn_in('build-appliance', [
                '--user=root',
                f'--serve-rpm-snapshot={snapshot_dir}',
                f'--forward-fd={yum_dnf_stdout.fileno()}',  # becomes FD 3
                f'--forward-fd={rpm_contents.fileno()}',  # becomes FD 4
                *extra_args,
                '--',
                '/bin/sh', '-c',
                textwrap.dedent(f'''\
                    set -ex
                    mkdir /target
                    {prog} \\
                        --config={snapshot_dir}/etc/{prog}/{prog}.conf \\
                        --installroot=/target -y install {package} |
                            tee /proc/self/fd/3
                    # We install only 1 RPM, so a glob tells us the filename.
                    # Use `head` instead of `cat` to fail nicer on exceeding 1.
                    head /target/usr/share/rpm_test/*.txt >&4
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

    def test_yum_with_repo_server(self):
        self._check_yum_dnf_boot_or_not(
            'yum',
            'rpm-test-carrot',
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                'carrot 2 rc0\n',
                b'Package rpm-test-carrot.x86_64 0:2-rc0 will be installed',
            ),
        )

    def test_dnf_with_repo_server(self):
        self._check_yum_dnf_boot_or_not(
            'dnf',
            'rpm-test-mice',
            check_ret_fn=functools.partial(
                self._check_yum_dnf_ret,
                'mice 0.1 a\n',
                b'Installing       : rpm-test-mice-0.1-a.x86_64',
            ),
        )

    @contextmanager
    def _write_versionlocks(self, lines: Iterable[str]):
        with tempfile.NamedTemporaryFile(mode='w') as tf:
            tf.write('\n'.join(lines) + '\n')
            tf.flush()
            yield tf.name

    def test_version_lock(self):
        # Version-locking carrot causes a non-latest version to be installed
        # -- compare with `test_yum_with_repo_server`.
        with self._write_versionlocks([
            '0\trpm-test-carrot\t1\tlockme\tx86_64'
        ]) as vl:
            self._check_yum_dnf_boot_or_not(
                'yum',
                'rpm-test-carrot',
                extra_args=(
                    '--snapshot-to-versionlock',
                    DEFAULT_SNAPSHOT_INSTALL_DIR,
                    vl,
                ),
                check_ret_fn=functools.partial(
                    self._check_yum_dnf_ret,
                    'carrot 1 lockme\n',
                    b'Package rpm-test-carrot.x86_64 0:1-lockme will be instal',
                ),
            )

        def _not_reached(ret):
            raise NotImplementedError

        with self._write_versionlocks([
            '0\trpm-test-carrot\t3333\tnonesuch\tx86_64'
        ]) as vl, self.assertRaises(subprocess.CalledProcessError):
            self._check_yum_dnf_boot_or_not(
                'yum',
                'rpm-test-carrot',
                extra_args=(
                    '--snapshot-to-versionlock',
                    DEFAULT_SNAPSHOT_INSTALL_DIR,
                    vl,
                ),
                check_ret_fn=_not_reached,
            )
