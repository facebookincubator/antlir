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
