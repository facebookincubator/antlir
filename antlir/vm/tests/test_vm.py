# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import contextlib
import os
import subprocess
import tempfile
import time

from antlir.fs_utils import Path, temp_dir
from antlir.tests.common import AntlirTestCase
from antlir.unshare import Namespace, Unshare
from antlir.vm.bzl.vm import vm_opts_t
from antlir.vm.vm import (
    _create_tpm,
    _wait_for_boot,
    DEFAULT_TIMEOUT_MS,
    ShellMode,
    vm,
    VMBootError,
    VMExecOpts,
)


class TestAntlirVM(AntlirTestCase):
    def test_parse_cli(self):
        with temp_dir() as td:
            opts_path = td / "opts.json"
            with opts_path.open(mode="w") as f:
                f.write(os.environ["test-vm-json"])

            opts_cli_arg = "--opts={}".format(opts_path)
            opts_instance = vm_opts_t.from_env("test-vm-json")

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

            # Test --append-console
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

    async def test_api_ssh(self):
        opts_instance = vm_opts_t.from_env("test-vm-json")

        async with vm(
            opts=opts_instance,
            console=2,
        ) as (instance, boottime_ms, timeout_ms):
            proc = await instance.run(
                cmd=["/bin/hostnamectl", "status", "--transient"],
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
                cmd=["test", "-S", "/tmp/test.sock"],
                timeout_ms=timeout_ms,
                forward={"/tmp/test.sock": "/tmp/test.sock"},
            )
            self.assertEqual(proc.returncode, 0)

    async def test_api_console(self):
        opts_instance = vm_opts_t.from_env("test-vm-json")

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

    async def test_api_kernel_panic(self):
        opts_instance = vm_opts_t.from_env("test-vm-json")

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

    async def test_api_sidecar(self):
        opts_instance = vm_opts_t.from_env("test-vm-sidecar-json")

        async with vm(
            opts=opts_instance,
            console=2,
        ) as (instance, boottime_ms, timeout_ms):
            pass

    async def test_api_initrd_fail(self):
        opts_instance = vm_opts_t.from_env("test-vm-initrd-fail-json")

        with tempfile.NamedTemporaryFile() as console_file:
            start = time.time()
            with self.assertRaisesRegex(RuntimeError, "VM failed to boot"):
                async with vm(
                    opts=opts_instance,
                    console=Path(console_file.name),
                    timeout_ms=DEFAULT_TIMEOUT_MS,
                ) as (instance, boottime_ms, timeout_ms):
                    pass
            elapsed_s = time.time() - start
            console_logs = console_file.read()
        self.assertLess(
            elapsed_s,
            DEFAULT_TIMEOUT_MS / 1000.0 / 3,
            "time to detect a failed boot should be significantly faster than the timeout",
        )
        # make sure it was actually a vm reboot, not an unrelated infra failure
        self.assertIn(b"reboot: machine restart", console_logs)

    async def test_wait_for_boot_success(self):
        async def _handle(r, w):
            w.write(b"READY\n")
            await w.drain()
            w.close()
            await w.wait_closed()

        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            server = await asyncio.start_unix_server(_handle, path=tempsock)

            async with server:
                await _wait_for_boot(tempsock, timeout_ms=10000)

    async def test_wait_for_boot_error(self):
        async def _handle(r, w):
            w.write(b"BAD")
            await w.drain()
            w.close()
            await w.wait_closed()

        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            server = await asyncio.start_unix_server(_handle, path=tempsock)

            with self.assertRaisesRegex(
                VMBootError, "Received invalid boot notification"
            ):
                async with server:
                    await _wait_for_boot(tempsock, timeout_ms=10000)

    async def test_wait_for_boot_eof(self):
        async def _handle(r, w):
            w.write(b"")
            await w.drain()
            w.close()
            await w.wait_closed()

        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            server = await asyncio.start_unix_server(_handle, path=tempsock)

            with self.assertRaisesRegex(
                VMBootError, "Received EOF before boot notification!"
            ):
                async with server:
                    await _wait_for_boot(tempsock, timeout_ms=10000)

    async def test_wait_for_boot_timeout_socket(self):
        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            with self.assertRaisesRegex(
                VMBootError, "Timeout waiting for notify socket"
            ):
                # The timeout_sec can be really fast because we know it
                # will never show up, so just do it as quick as we can.
                await _wait_for_boot(tempsock, timeout_ms=10)

    async def test_wait_for_boot_timeout_on_recv(self):
        # This does nothing to force a timeout event
        async def _handle(r, w):
            pass

        with tempfile.TemporaryDirectory() as td:
            tempsock = Path(td) / "temp.sock"

            server = await asyncio.start_unix_server(_handle, path=tempsock)

            async with server:
                with self.assertRaisesRegex(
                    VMBootError, "Timeout waiting for boot notify"
                ):
                    await _wait_for_boot(tempsock, timeout_ms=100)

    async def test_create_tpm_timeout(self):
        timeout = 50  # ms

        async with contextlib.AsyncExitStack() as stack:
            with Unshare([Namespace.NETWORK, Namespace.PID]) as ns:
                with self.assertRaises(VMBootError):
                    await _create_tpm(stack, ns, "/bin/ls", timeout)

    async def test_disk_interface_virtio(self):
        opts = vm_opts_t.from_env("test-vm-json")

        async with vm(
            opts=opts,
            console=2,
        ) as (instance, boottime_ms, timeout_ms):
            proc = await instance.run(
                cmd=["/bin/ls", "/dev/vda"],
                timeout_ms=timeout_ms,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            stdout, _ = await proc.communicate()
            self.assertEqual(proc.returncode, 0)

    async def test_disk_interface_nvme(self):
        opts = vm_opts_t.from_env("test-vm-nvme-json")

        async with vm(
            opts=opts,
            console=2,
        ) as (instance, boottime_ms, timeout_ms):
            proc = await instance.run(
                cmd=["/bin/ls", "/dev/nvme0n1"],
                timeout_ms=timeout_ms,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            stdout, _ = await proc.communicate()
            self.assertEqual(proc.returncode, 0)

    async def test_disk_interface_sata(self):
        opts = vm_opts_t.from_env("test-vm-sata-json")

        async with vm(
            opts=opts,
            console=2,
        ) as (instance, boottime_ms, timeout_ms):
            proc = await instance.run(
                cmd=["/bin/ls", "/dev/sda"],
                timeout_ms=timeout_ms,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            stdout, _ = await proc.communicate()
            self.assertEqual(proc.returncode, 0)
