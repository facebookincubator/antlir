# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import socket
import tempfile
import threading
import unittest

from antlir.fs_utils import Path
from antlir.vm.vm import _wait_for_boot, ShellMode, VMBootError, VMExecOpts
from antlir.vm.vm_opts_t import vm_opts_t


class TestAntlirVM(unittest.TestCase):
    def test_wait_for_boot_success(self):
        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            try:
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sock.bind(tempsock)

                sock.listen(1)

                t = threading.Timer(0.01, _wait_for_boot, [tempsock])
                t.start()

                # This is going to block until at least one client connects
                conn, _ = sock.accept()
                try:
                    # Send an empty string
                    conn.sendall(b"")
                finally:
                    conn.close()
                    # Just in case this races with the timer
                    t.cancel()

            finally:
                sock.close()

    def test_wait_for_boot_timeout_on_socket(self):
        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            with self.assertRaisesRegex(
                VMBootError, "Timeout waiting for notify socket"
            ):
                # The timeout_sec can be really fast because we know it
                # will never show up, so just do it as quick as we can.
                _wait_for_boot(tempsock, timeout_ms=10)

    def test_wait_for_boot_timeout_on_recv(self):
        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            try:
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                sock.bind(tempsock)

                sock.listen(1)

                # To wait in the main thread for the timeout to happen
                # instead of having to sleep and risk a race condition
                # on oversubscribed test infra.
                barrier = threading.Barrier(2)

                # wrap the assert so that we can catch the exception in
                # the Timer, otherwise it will not get propagated
                # properly.
                def _catch_timeout_in_thread():
                    with self.assertRaisesRegex(
                        VMBootError, "Timeout waiting for boot event"
                    ):
                        _wait_for_boot(tempsock, timeout_ms=100)

                    barrier.wait()

                t = threading.Timer(0.01, _catch_timeout_in_thread)
                t.start()

                # This is going to block until at least one client connects
                conn, _ = sock.accept()
                try:
                    # do nothing, we want the timeout to happen
                    barrier.wait()
                finally:
                    conn.close()
                    # Just in case this races with the timer
                    t.cancel()

            finally:
                sock.close()

    def test_parse_cli(self):
        opts_instance = vm_opts_t.from_env("test-vm-json")
        opts_cli_arg = "--opts={}".format(os.environ["test-vm-json"])
        # Test defaults of everything that has a default
        self.assertEqual(
            VMExecOpts(
                opts=opts_instance,
            ),
            VMExecOpts.parse_cli(
                [
                    opts_cli_arg,
                ]
            ),
            opts_instance,
        )

        # Test extra, debug, shell mode as console
        self.assertEqual(
            VMExecOpts(
                opts=opts_instance,
                debug=True,
                shell=ShellMode.console,
                extra=["--extra-argument"],
            ),
            VMExecOpts.parse_cli(
                [
                    opts_cli_arg,
                    "--debug",
                    "--shell=console",
                    "--extra-argument",
                ]
            ),
        )

        # Test --console
        self.assertEqual(
            VMExecOpts(
                opts=opts_instance,
                console=None,
            ),
            VMExecOpts.parse_cli(
                [
                    opts_cli_arg,
                    "--console",
                ]
            ),
        )

        # Test --console=/path/to/something
        with tempfile.NamedTemporaryFile() as t:
            t = Path(t.name)
            self.assertEqual(
                VMExecOpts(
                    opts=opts_instance,
                    console=t,
                ),
                VMExecOpts.parse_cli([opts_cli_arg, "--console={}".format(t)]),
            )
