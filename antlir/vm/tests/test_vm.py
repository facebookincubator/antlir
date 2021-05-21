# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import os
import socket
import subprocess
import tempfile
import threading
import unittest

from antlir.fs_utils import Path
from antlir.vm.vm import _wait_for_boot, ShellMode, VMBootError, VMExecOpts, vm
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
        opts_instance = vm_opts_t.from_env("test-vm-agent-json")
        opts_cli_arg = "--opts={}".format(os.environ["test-vm-agent-json"])
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
                    "--append-console",
                ]
            ),
        )

        # Test --append-console=/path/to/something
        with tempfile.NamedTemporaryFile() as t:
            t = Path(t.name)
            self.assertEqual(
                VMExecOpts(
                    opts=opts_instance,
                    console=t,
                ),
                VMExecOpts.parse_cli(
                    [opts_cli_arg, "--append-console={}".format(t)]
                ),
            )

        # Test --shell=ssh
        self.assertEqual(
            VMExecOpts(
                opts=opts_instance,
                shell=ShellMode.ssh,
            ),
            VMExecOpts.parse_cli(
                [
                    opts_cli_arg,
                    "--shell=ssh",
                ]
            ),
        )


class AsyncTestAntlirVm(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # Needed for the async tests
        cls.event_loop = asyncio.new_event_loop()
        asyncio.set_event_loop(cls.event_loop)

    @classmethod
    def tearDownClass(cls):
        cls.event_loop.close()

    async def _test_vm_scheme(self, scheme):
        opts_instance = vm_opts_t.from_env(f"test-vm-{scheme}-json")
        async with vm(
            opts=opts_instance,
        ) as (instance, boottime_ms, timeout_ms):
            retcode, stdout, _ = await instance.run(
                cmd=["/bin/hostnamectl", "status", "--static"],
                timeout_ms=timeout_ms,
            )

            self.assertEqual(stdout, b"vmtest\n")
            self.assertEqual(retcode, 0)

            retcode, stdout, _ = await instance.run(
                cmd=["pwd"],
                cwd="/tmp",
                timeout_ms=timeout_ms,
            )
            self.assertEqual(stdout, b"/tmp\n")
            self.assertEqual(retcode, 0)

            with self.assertRaises(subprocess.CalledProcessError):
                await instance.run(
                    check=True,
                    cmd=["/bin/false"],
                    timeout_ms=timeout_ms,
                )

    def test_api_scheme_ssh(self):
        self.event_loop.run_until_complete(self._test_vm_scheme(scheme="ssh"))

    def test_api_console(self):
        opts_instance = vm_opts_t.from_env("test-vm-ssh-json")

        async def _test():
            with tempfile.NamedTemporaryFile() as tf:
                async with vm(
                    opts=opts_instance,
                    console=Path(tf.name),
                ) as (instance, boottime_ms, timeout_ms):
                    # We only care about capturing console output
                    retcode, _, _ = await instance.run(
                        [
                            "bash",
                            "-c",
                            r"""'echo "TEST CONSOLE" > /dev/console'""",
                        ],
                        check=True,
                        timeout_ms=timeout_ms,
                    )

                self.assertIn(b"TEST CONSOLE", tf.read())

        self.event_loop.run_until_complete(_test())

    def test_api_kernel_panic(self):
        opts_instance = vm_opts_t.from_env("test-vm-ssh-json")

        async def _test():
            with tempfile.NamedTemporaryFile() as console_f:
                async with vm(
                    opts=opts_instance,
                    console=Path(console_f.name),
                ) as (instance, boottime_ms, timeout_ms):
                    # We only care about capturing console output
                    retcode, _, _ = await instance.run(
                        [
                            "bash",
                            "-c",
                            r"""'echo c > /proc/sysrq-trigger'""",
                        ],
                        # This is expected to fail
                        check=False,
                        timeout_ms=timeout_ms,
                    )

                # Expect to see the kernel panic message in the console output
                self.assertIn(
                    b"Kernel panic - not syncing: sysrq triggered crash",
                    console_f.read(),
                )
                # Expect that we failed with 255, which is the error returned by
                # SSH when it encounters an error.
                self.assertEqual(retcode, 255)

        self.event_loop.run_until_complete(_test())

    def test_api_sidecar(self):
        opts_instance = vm_opts_t.from_env("test-vm-sidecar-json")

        async def _test():
            async with vm(
                opts=opts_instance,
            ) as (instance, boottime_ms, timeout_ms):
                pass

        self.event_loop.run_until_complete(_test())
