#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import tempfile
import threading
from contextlib import contextmanager
from pwd import struct_passwd
from unittest import mock

from antlir.artifacts_dir import find_buck_cell_root
from antlir.common import pipe
from antlir.find_built_subvol import find_built_subvol
from antlir.tests.layer_resource import layer_resource
from antlir.tests.temp_subvolumes import with_temp_subvols

from ..args import _QUERY_TARGETS_AND_OUTPUTS_SEP, _parse_cli_args
from ..cmd import _colon_quote_path, _extra_nspawn_args_and_env
from ..common import DEFAULT_PATH_ENV
from .base import (
    NspawnTestBase,
    _mocks_for_extra_nspawn_args,
    _mocks_for_parse_cli_args,
)


class NspawnTestCase(NspawnTestBase):
    def _assertIsSubseq(self, subseq, seq, msg=None):
        subseqs = [
            seq[i : i + len(subseq)] for i in range(len(seq) - len(subseq) + 1)
        ]
        self.assertIn(subseq, subseqs)

    def test_extra_nspawn_args_private_network_opts(self):
        # opts.private_network
        self.assertIn(
            "--private-network",
            self._wrapper_args_to_nspawn_args(["--layer", "test"]),
        )
        # !opts.private_network
        self.assertNotIn(
            "--private-network",
            self._wrapper_args_to_nspawn_args(
                ["--layer", "test", "--no-private-network"]
            ),
        )

    def test_extra_nspawn_args_bindmount_opts(self):
        # opts.bindmount_rw
        self._assertIsSubseq(
            ["--bind", "/src:/dest"],
            self._wrapper_args_to_nspawn_args(
                ["--layer", "test", "--bindmount-rw", "/src", "/dest"]
            ),
        )
        # opts.bindmount_ro
        self._assertIsSubseq(
            ["--bind-ro", "/src:/dest"],
            self._wrapper_args_to_nspawn_args(
                ["--layer", "test", "--bindmount-ro", "/src", "/dest"]
            ),
        )

    @mock.patch("antlir.config.find_buck_cell_root")
    def test_extra_nspawn_args_bind_repo_opts(self, root_mock):
        root_mock.return_value = "/repo/root"
        # opts.bind_repo_ro
        self.assertIn(
            "/repo/root:/repo/root",
            self._wrapper_args_to_nspawn_args(
                ["--layer", "test", "--bind-repo-ro"]
            ),
        )
        # artifacts_require_repo
        self.assertIn(
            "/repo/root:/repo/root",
            self._wrapper_args_to_nspawn_args(
                ["--layer", "test"], artifacts_require_repo=True
            ),
        )

    def test_extra_nspawn_args_log_tmpfs_opts(self):
        base_argv = ["--layer", "test", "--user=is_mocked"]
        # opts.logs_tmpfs
        self.assertIn(
            "--tmpfs=/logs:uid=123,gid=123,mode=0755,nodev,nosuid,noexec",
            self._wrapper_args_to_nspawn_args(base_argv),
        )
        # !opts.logs_tmpfs
        self.assertNotIn(
            "--tmpfs=/logs:uid=123,gid=123,mode=0755,nodev,nosuid,noexec",
            self._wrapper_args_to_nspawn_args(base_argv + ["--no-logs-tmpfs"]),
        )

    def test_extra_nspawn_args_cap_net_admin_opts(self):
        # opts.cap_net_admin
        self.assertIn(
            "--capability=CAP_NET_ADMIN",
            self._wrapper_args_to_nspawn_args(
                ["--layer", "test", "--cap-net-admin"]
            ),
        )

    def test_extra_nspawn_args_hostname_opts(self):
        # opts.hostname
        self.assertIn(
            "--hostname=test-host",
            self._wrapper_args_to_nspawn_args(
                ["--layer", "test", "--hostname", "test-host"]
            ),
        )

    def test_extra_nspawn_args_quiet_opts(self):
        # opts.quiet
        self.assertIn(
            "--quiet",
            self._wrapper_args_to_nspawn_args(["--layer", "test", "--quiet"]),
        )

    def test_extra_nspawn_args_foward_tls_env_opts(self):
        # opts.foward_tls_env
        with _mocks_for_parse_cli_args():
            args = _parse_cli_args(
                ["--layer", "test", "--forward-tls-env"],
                allow_debug_only_opts=True,
            )
        with _mocks_for_extra_nspawn_args(
            artifacts_require_repo=False
        ), mock.patch.dict(os.environ, {"THRIFT_TLS_TEST": "test_val"}):
            _nspawn_args, cmd_env = _extra_nspawn_args_and_env(args.opts)
            self.assertIn("THRIFT_TLS_TEST=test_val", cmd_env)

    def test_exit_code(self):
        self.assertEqual(
            37,
            self._nspawn_in(
                (__package__, "test-layer"),
                ["--", "sh", "-c", "exit 37"],
                check=False,
            ).returncode,
        )

    def test_redirects(self):
        cmd = ["--", "sh", "-c", "echo ohai && echo abracadabra >&2"]
        ret = self._nspawn_in(
            (__package__, "test-layer"),
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.assertEqual(b"ohai\n" + self.maybe_extra_ending, ret.stdout)

        # stderr is not just a clean `abracadabra\n` because we don't
        # suppress nspawn's debugging output, hence the 'assertIn'.
        self.assertIn(b"abracadabra\n", ret.stderr)

        # The same test with `--quiet` is much simpler.
        ret = self._nspawn_in(
            (__package__, "test-layer"),
            ["--quiet"] + cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.assertEqual(b"ohai\n", ret.stdout)
        target_stderr = b"abracadabra\n"
        if self.nspawn_version.major >= 244:
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
        self._nspawn_in(
            (__package__, "bootable-systemd-os"),
            [
                "--",
                "sh",
                "-uexc",
                # Ensure the machine-id file exists and is empty.
                "test -e /etc/machine-id -a ! -s /etc/machine-id",
            ],
        )

    def test_logs_directory(self):
        # The log directory is on by default.
        ret = self._nspawn_in(
            (__package__, "test-layer"),
            [
                "--",
                "sh",
                "-c",
                'touch /logs/foo && stat --format="%U %G %a" /logs && whoami',
            ],
            stdout=subprocess.PIPE,
        )
        self.assertEqual(0, ret.returncode)
        self.assertEqual(
            b"nobody nobody 755\nnobody\n" + self.maybe_extra_ending, ret.stdout
        )
        # And the option prevents it from being created.
        self.assertEqual(
            0,
            self._nspawn_in(
                (__package__, "test-layer"),
                ["--no-logs-tmpfs", "--", "test", "!", "-e", "/logs"],
            ).returncode,
        )

    def test_forward_fd(self):
        with tempfile.TemporaryFile() as tf:
            tf.write(b"hello")
            tf.seek(0)
            ret = self._nspawn_in(
                (__package__, "test-layer"),
                [
                    "--forward-fd",
                    str(tf.fileno()),
                    "--",
                    "sh",
                    "-c",
                    "cat <&3 && echo goodbye >&3",
                ],
                stdout=subprocess.PIPE,
            )
            self.assertEqual(b"hello" + self.maybe_extra_ending, ret.stdout)
            tf.seek(0)
            self.assertEqual(b"hellogoodbye\n", tf.read())

    @with_temp_subvols
    def test_non_ephemeral_snapshot(self, temp_subvols):
        dest_subvol = temp_subvols.caller_will_create("persistent")
        # We won't create this subvol by manipulating this very object, but
        # rather indirectly through its path.  So its _exists would never
        # get updated, which would cause the TempSubvolumes cleanup to fail.
        # Arguably, the cleanup should be robust to this, but since this is
        # the unique place we have to do it, keep it simple.
        dest_subvol._exists = True
        self._nspawn_in(
            (__package__, "test-layer"),
            [
                f"--snapshot-into={dest_subvol.path()}",
                "--",
                # Also tests that we are a non-root user in the container.
                "sh",
                "-c",
                'echo ohaibai "$USER" > /home/nobody/poke',
            ],
        )
        with open(dest_subvol.path("/home/nobody/poke")) as f:
            self.assertEqual("ohaibai nobody\n", f.read())
        # Spot-check: the host mounts should still be available on the snapshot
        self.assertTrue(os.path.exists(dest_subvol.path("/bin/bash")))

    def test_bind_repo(self):
        self._nspawn_in(
            (__package__, "test-layer"),
            [
                "--bind-repo-ro",
                "--",
                "grep",
                "supercalifragilisticexpialidocious",
                os.path.join(
                    os.path.realpath(find_buck_cell_root()),
                    "antlir/nspawn_in_subvol/tests",
                    os.path.basename(__file__),
                ),
            ],
        )

    def test_cap_net_admin(self):
        self._nspawn_in(
            (__package__, "test-layer"),
            [
                "--user",
                "root",
                "--no-private-network",
                "--cap-net-admin",
                "--",
                "unshare",
                "--net",
                "ip",
                "link",
                "set",
                "dev",
                "lo",
                "up",
            ],
        )

    def test_hostname(self):
        ret = self._nspawn_in(
            (__package__, "test-layer"),
            ["--hostname=test-host.com", "--", "/bin/hostname"],
            stdout=subprocess.PIPE,
            check=True,
        )
        self.assertEqual(b"test-host.com\n", ret.stdout)

    @mock.patch.dict(
        "os.environ",
        {"THRIFT_TLS_KITTEH": "meow", "UNENCRYPTED_KITTEH": "woof"},
    )
    def test_tls_environment(self):
        ret = self._nspawn_in(
            (__package__, "test-layer"),
            [
                "--forward-tls-env",
                "--",
                "printenv",
                "THRIFT_TLS_KITTEH",
                "UNENCRYPTED_KITTEH",
            ],
            stdout=subprocess.PIPE,
            check=False,
        )
        self.assertNotEqual(0, ret.returncode)  # UNENCRYPTED_KITTEH is unset
        self.assertEqual(b"meow\n" + self.maybe_extra_ending, ret.stdout)

    def test_bindmount_rw(self):
        with tempfile.TemporaryDirectory() as tmpdir, tempfile.TemporaryDirectory() as tmpdir2:  # noqa: E501
            self._nspawn_in(
                (__package__, "test-layer"),
                [
                    "--user",
                    "root",
                    "--bindmount-rw",
                    tmpdir,
                    "/tmp",
                    "--bindmount-rw",
                    tmpdir2,
                    "/mnt",
                    "--",
                    "touch",
                    "/tmp/testfile",
                    "/mnt/testfile",
                ],
            )
            self.assertTrue(os.path.isfile(f"{tmpdir}/testfile"))
            self.assertTrue(os.path.isfile(f"{tmpdir2}/testfile"))

    def test_bindmount_ro(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with self.assertRaises(subprocess.CalledProcessError):
                ret = self._nspawn_in(
                    (__package__, "test-layer"),
                    [
                        "--user",
                        "root",
                        "--bindmount-ro",
                        tmpdir,
                        "/tmp",
                        "--",
                        "touch",
                        "/tmp/testfile",
                    ],
                )
                self.assertEqual(
                    "touch: cannot touch '/tmp/testfile': "
                    + "Read-only file system",
                    ret.stdout,
                )

    def test_xar(self):
        "Make sure that XAR binaries work in vanilla `buck run` containers"
        ret = self._nspawn_in(
            (__package__, "host-hello-xar"),
            ["--", "/hello.xar"],
            stdout=subprocess.PIPE,
            check=True,
        )
        self.assertEqual(b"hello world\n" + self.maybe_extra_ending, ret.stdout)

    def test_mknod(self):
        "CAP_MKNOD is dropped by our runtime."
        ret = self._nspawn_in(
            (__package__, "test-layer"),
            ["--user", "root", "--quiet", "--", "mknod", "/foo", "c", "1", "3"],
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertNotEqual(0, ret.returncode)
        self.assertRegex(
            ret.stderr, b"mknod: (|')/foo(|'): Operation not permitted\n"
        )

    def test_boot_cmd_is_system_running(self):
        ret = self._nspawn_in(
            (__package__, "bootable-systemd-os"),
            [
                "--boot",
                # This needs to be root because we don't yet create a proper
                # login session for non-privileged users when we execute
                # commands. Systemctl will try and connect to the user session
                # when it's run as non-root.
                "--user=root",
                "--",
                "/usr/bin/systemctl",
                "is-system-running",
                "--wait",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        # The assertions here are validating that the systemd instance inside
        # the container completed its boot but *not* that it successfully
        # started every service.  This reason for this is that it's not
        # a gauranteed property of the image build system that the image
        # successfully boot systemd, but simply that systemd can be properly
        # started and 'complete' a boot.  The success of the boot is really
        # something that can only be properly evaulated by unit tests for
        # a specific image.
        self.assertIn(ret.returncode, [0, 1], msg=ret.stderr.strip())
        self.assertIn(
            ret.stdout.strip(),
            [b"running", b"degraded"],
            msg=ret.stderr.strip(),
        )
        # versions < 244 did not properly respect --quiet
        if self.nspawn_version.major >= 244:
            self.assertEqual(
                [b""],
                [
                    l
                    for l in ret.stderr.split(b"\n")
                    if not l.startswith(b"DEBUG recv_fds_and_run.py")
                ],
            )

    def test_boot_cmd_failure(self):
        ret = self._nspawn_in(
            (__package__, "bootable-systemd-os"),
            ["--boot", "--", "/usr/bin/false"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(1, ret.returncode)
        self.assertEqual(b"", ret.stdout)
        # versions < 244 did not properly respect --quiet
        if self.nspawn_version.major >= 244:
            self.assertEqual(b"", ret.stderr)

    def test_boot_forward_fd(self):
        with tempfile.TemporaryFile() as tf:
            tf.write(b"hello")
            tf.seek(0)
            ret = self._nspawn_in(
                (__package__, "bootable-systemd-os"),
                [
                    "--boot",
                    "--forward-fd",
                    str(tf.fileno()),
                    "--",
                    "/usr/bin/sh",
                    "-c",
                    "/usr/bin/cat <&3 && /usr/bin/echo goodbye >&3",
                ],
                stdout=subprocess.PIPE,
                check=True,
            )
            self.assertEqual(b"hello", ret.stdout)
            tf.seek(0)
            self.assertEqual(b"hellogoodbye\n", tf.read())

    def test_boot_unprivileged_user(self):
        ret = self._nspawn_in(
            (__package__, "bootable-systemd-os"),
            ["--boot", "--", "/bin/whoami"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
        )
        self.assertEqual(0, ret.returncode)
        self.assertEqual(b"nobody\n", ret.stdout)
        self.assertEqual(b"", ret.stderr)

    def test_boot_env_clean(self):
        ret = self._nspawn_in(
            (__package__, "bootable-systemd-os"),
            ["--boot", "--", "/bin/env"],
            stdout=subprocess.PIPE,
            check=True,
        )
        self.assertEqual(0, ret.returncode)

        # Verify we aren't getting anything in from the outside we don't want
        self.assertNotIn(b"BUCK_BUILD_ID", ret.stdout)

        # Verify we get what we expect
        self.assertIn(b"HOME", ret.stdout)
        self.assertIn(b"PATH", ret.stdout)
        self.assertIn(b"LOGNAME", ret.stdout)
        self.assertIn(b"USER", ret.stdout)
        self.assertIn(b"TERM=linux-clown", ret.stdout)

    def test_boot_proc_results(self):
        console_singleton = []
        with pipe() as (r, w):

            def read_console():
                nonlocal console_singleton
                console_singleton.append(r.read())

            # We have to consume the read end of the pipe from a thread
            # because otherwise `systemd` could deadlock during shutdown.
            # This is explained in the `nspawn.py` docblock.
            reader_thread = threading.Thread(target=read_console)
            reader_thread.start()

            ret, ret_boot = self._nspawn_in_boot_ret(
                (__package__, "bootable-systemd-os"),
                ["--boot", "--", "/bin/true"],
                console=w,
                check=True,
            )
        reader_thread.join()

        self.assertEqual(0, ret.returncode)

        self.assertIsNotNone(ret_boot)
        self.assertIsNotNone(ret_boot.returncode)
        self.assertEqual(0, ret_boot.returncode)
        (console,) = console_singleton
        self.assertIn(b"Welcome to", console)
        self.assertIn(b"Reached target", console)
        self.assertIn(b"Stopped target", console)

    def test_boot_error(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self._nspawn_in(
                (__package__, "bootable-systemd-os"),
                ["--boot", "--", "/bin/false"],
            )

    def test_boot_no_console_pipe(self):
        with self.assertRaisesRegex(
            RuntimeError, " does not support `subprocess.PIPE` "
        ):
            # This is identical to `test_boot_proc_results` except for the pipe
            self._nspawn_in(
                (__package__, "bootable-systemd-os"),
                ["--boot", "--", "/bin/true"],
                console=subprocess.PIPE,
                check=True,
            )

    # The default path determines which binaries get shadowed, so it's
    # important that it be the same across the board.
    def test_path_env(self):
        for layer in ["test-layer", "bootable-systemd-os"]:
            for extra_args, expected_path in [
                [[], DEFAULT_PATH_ENV],
                [["--user=root"], DEFAULT_PATH_ENV],
                [["--setenv=PATH=/foo:/bin"], b"/foo:/bin"],
            ]:
                with self.subTest((layer, extra_args)):
                    self.assertEqual(
                        expected_path + b"\n",
                        self._nspawn_in(
                            (__package__, layer),
                            [*extra_args, "--", "printenv", "PATH"],
                            stdout=subprocess.PIPE,
                        ).stdout,
                    )

    def test_mount_args(self):
        argv = [
            "--layer",
            layer_resource(__package__, "test-layer-with-mounts"),
            "--layer-dep-to-location",
            "{target}{sep}{location}".format(
                location=layer_resource(__package__, "test-hello-world-base"),
                sep=_QUERY_TARGETS_AND_OUTPUTS_SEP,
                target="//antlir/compiler/test_images:hello_world_base",
            ),
        ]

        args = _parse_cli_args(argv, allow_debug_only_opts=False)
        args, _env = _extra_nspawn_args_and_env(args.opts)

        # Verify that host mounts are properly setup as
        # --bind-ro args to nspawn
        self._assertIsSubseq(["--bind-ro", "/dev/null:/dev_null"], args)
        self._assertIsSubseq(["--bind-ro", "/etc:/host_etc"], args)

        # Verify that layer mounts are properly setup as
        # --bind-ro args to nspawn
        self._assertIsSubseq(
            [
                "--bind-ro",
                "{subvol}:{mount}".format(
                    subvol=_colon_quote_path(
                        find_built_subvol(
                            layer_resource(__package__, "test-hello-world-base")
                        ).path()
                    ),
                    mount="/meownt",
                ),
            ],
            args,
        )

    def test_mounted_mounts(self):
        expected_mounts = [b"/dev_null", b"/host_etc", b"/meownt"]

        with tempfile.TemporaryFile() as tf:
            self._nspawn_in(
                (__package__, "test-layer-with-mounts"),
                [
                    "--layer-dep-to-location",
                    "{target}{sep}{location}".format(
                        location=layer_resource(
                            __package__, "test-hello-world-base"
                        ),
                        sep=_QUERY_TARGETS_AND_OUTPUTS_SEP,
                        target=(
                            "//antlir/compiler/test_images:" "hello_world_base"
                        ),
                    ),
                    "--forward-fd",
                    str(tf.fileno()),
                    "--",
                    "/usr/bin/sh",
                    "-c",
                    "/usr/bin/mount >&3",
                ],
                check=True,
            )
            tf.seek(0)

            # Split the mount lines
            splits = [line.split() for line in tf.readlines()]

            # Verify that we have the mounts that we expect.  Note:
            # We don't validate that the 'source' of the mount
            # is correct here for 2 reasons:
            #  - The fact that the source is set properly in the meta
            #    is tested elsewhere in the mount tests.
            #  - We validate that the `--bind-ro` option passed to
            #   `systemd-nspawn is properly constructed for a mount.
            #  - We don't always get the 'source' of the mount in the
            #    output of the `mount` command.  When the source exists
            #    on a btrfs volume we can use the `subvol=` option, but
            #    we can't always ensure in the case of a host mount that
            #    the host is actually running on btrfs.
            self.assertTrue(
                set(expected_mounts).issubset([split[2] for split in splits])
            )
