#!/usr/bin/env python3
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest

from contextlib import contextmanager
from pwd import struct_passwd
from unittest import mock

from artifacts_dir import find_repo_root
from fs_image.nspawn_in_subvol.args import _parse_cli_args
from fs_image.nspawn_in_subvol.common import _nspawn_version
from fs_image.nspawn_in_subvol.cmd import _extra_nspawn_args_and_env
from fs_image.nspawn_in_subvol.run import nspawn_in_subvol
from tests.temp_subvolumes import with_temp_subvols


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
        # Setup expected stdout line endings depending on the version
        # of systemd-nspawn.  Version 242 'fixed' stdout line endings.
        # The extra newline for versions < 242 is due to T40936918 mentioned
        # in `nspawn_in_subvol.py`.  It would disappear if we passed `--quiet`
        # to nspawn, but we want to retain the extra debug logging.
        self.nspawn_version = _nspawn_version()
        self.maybe_extra_ending = b'\n' if self.nspawn_version < 242 else b''

    def _nspawn_in(self, rsrc_name, argv, **kwargs):
        args = _parse_cli_args([
            # __file__ works in @mode/opt since the resource is inside the XAR
            '--layer', os.path.join(os.path.dirname(__file__), rsrc_name),
        ] + argv, allow_debug_only_opts=True)
        return nspawn_in_subvol(args.opts, boot=args.boot, **kwargs)

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
            '--snapshot-into', dest_subvol.path().decode(), '--',
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
            'unshare', '--net', 'ifconfig', 'lo', 'up',
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
        ret = self._nspawn_in('bootable-systemd-os', [
            '--boot',
            '--',
            '/bin/true',
        ], stdout=subprocess.PIPE, check=True)
        self.assertEqual(0, ret.returncode)

        self.assertIsNotNone(ret.boot)
        self.assertIsNotNone(ret.boot.returncode)
        self.assertEqual(0, ret.boot.returncode)
        self.assertIn(b'Welcome to', ret.boot.stdout)
        self.assertIn(b'Reached target', ret.boot.stdout)
        self.assertIn(b'Stopped target', ret.boot.stdout)

    def _run_yum_or_dnf(self, progname, package,
        expected_filename, expected_contents, expected_logline,
    ):
        ret = self._nspawn_in('build-appliance', [
            '--user=root',
            '--serve-rpm-snapshot-dir',
            os.path.join(os.path.dirname(__file__), 'repo-snapshot'),
            '--',
            '/bin/sh', '-c',
            textwrap.dedent(f'''\
                set -ex
                mkdir /target
                {progname} --config=/repo-server/{progname}.conf \\
                        --installroot=/target -y install {package}
                test -f /target{expected_filename}
                contents=$(cat /target{expected_filename})
                test "$contents" = "{expected_contents}"
            '''),
        ], stdout=subprocess.PIPE, check=True)
        self.assertEqual(0, ret.returncode)
        self.assertIn(expected_logline, ret.stdout)
        self.assertIn(b'Complete!', ret.stdout)

    def test_yum_with_repo_server(self):
        self._run_yum_or_dnf(
            'yum',
            'rpm-test-carrot',
            '/usr/share/rpm_test/carrot.txt',
            'carrot 2 rc0',
            b'---> Package rpm-test-carrot.x86_64 0:2-rc0 will be installed',
        )

    def test_dnf_with_repo_server(self):
        self._run_yum_or_dnf(
            'dnf',
            'rpm-test-mice',
            '/usr/share/rpm_test/mice.txt',
            'mice 0.1 a',
            b'Installing       : rpm-test-mice-0.1-a.x86_64',
        )
