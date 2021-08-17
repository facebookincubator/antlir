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

    def test_api_ssh(self):
        opts_instance = vm_opts_t.from_env("test-vm-json")

        async def _test():
            async with vm(
                opts=opts_instance,
            ) as (instance, boottime_ms, timeout_ms):
                proc = await instance.run(
                    cmd=["/bin/hostnamectl", "status", "--static"],
                    timeout_ms=timeout_ms,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                stdout, _ = await proc.communicate()
                self.assertEqual(stdout, b"vmtest\n")
                self.assertEqual(proc.returncode, 0)

                proc = await instance.run(
                    cmd=["pwd"],
                    cwd="/tmp",
                    timeout_ms=timeout_ms,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                stdout, _ = await proc.communicate()
                self.assertEqual(stdout, b"/tmp\n")
                self.assertEqual(proc.returncode, 0)

                with self.assertRaises(subprocess.CalledProcessError):
                    await instance.run(
                        check=True,
                        cmd=["/bin/false"],
                        timeout_ms=timeout_ms,
                    )

                proc = await instance.run(
                    cmd=["/bin/file", "/tmp/test.sock"],
                    timeout_ms=timeout_ms,
                    forward={"/tmp/test.sock": "/tmp/test.sock"},
                )
                self.assertEqual(proc.returncode, 0)

        self.event_loop.run_until_complete(_test())

    def test_api_console(self):
        opts_instance = vm_opts_t.from_env("test-vm-json")

        async def _test():
            with tempfile.NamedTemporaryFile() as tf:
                async with vm(
                    opts=opts_instance,
                    console=Path(tf.name),
                ) as (instance, boottime_ms, timeout_ms):
                    # We only care about capturing console output
                    await instance.run(
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
        opts_instance = vm_opts_t.from_env("test-vm-json")

        async def _test():
            with tempfile.NamedTemporaryFile() as console_f:
                async with vm(
                    opts=opts_instance,
                    console=Path(console_f.name),
                ) as (instance, boottime_ms, timeout_ms):
                    # We only care about capturing console output
                    proc = await instance.run(
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
                self.assertEqual(proc.returncode, 255)

        self.event_loop.run_until_complete(_test())

    def test_api_sidecar(self):
        opts_instance = vm_opts_t.from_env("test-vm-sidecar-json")

        async def _test():
            async with vm(
                opts=opts_instance,
            ) as (instance, boottime_ms, timeout_ms):
                pass

        self.event_loop.run_until_complete(_test())

    def test_wait_for_boot_success(self):
        async def _handle(r, w):
            w.write(b"READY\n")
            await w.drain()
            w.close()
            await w.wait_closed()

        async def _test():
            with tempfile.TemporaryDirectory() as td:
                tempsock = Path(td) / "temp.sock"

                server = await asyncio.start_unix_server(_handle, path=tempsock)

                async with server:
                    await _wait_for_boot(tempsock, timeout_ms=10000)

        self.event_loop.run_until_complete(_test())

    def test_wait_for_boot_timeout_socket(self):
        async def _test():
            with tempfile.TemporaryDirectory() as td:
                tempsock = Path(td) / "temp.sock"

                with self.assertRaisesRegex(
                    VMBootError, "Timeout waiting for notify socket"
                ):
                    # The timeout_sec can be really fast because we know it
                    # will never show up, so just do it as quick as we can.
                    await _wait_for_boot(tempsock, timeout_ms=10)

        self.event_loop.run_until_complete(_test())

    def test_wait_for_boot_timeout_on_recv(self):
        # This does nothing to force a timeout event
        async def _handle(r, w):
            pass

        async def _test():
            with tempfile.TemporaryDirectory() as td:
                tempsock = Path(td) / "temp.sock"

                server = await asyncio.start_unix_server(_handle, path=tempsock)

                async with server:
                    with self.assertRaisesRegex(
                        VMBootError, "Timeout waiting for boot notify"
                    ):
                        await _wait_for_boot(tempsock, timeout_ms=100)

        self.event_loop.run_until_complete(_test())
