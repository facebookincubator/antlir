#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import logging
import subprocess
import time
import unittest
import unittest.mock
from typing import List, Union

from antlir.common import async_run_shell

from ..common import (
    async_retry_fn,
    async_retryable,
    async_run,
    kernel_version,
    log as common_log,
    retry_fn,
    retryable,
)


class TestCommon(unittest.IsolatedAsyncioTestCase):
    def test_retry_fn(self) -> None:
        class Retriable:
            def __init__(self, attempts_to_fail=0):
                self.attempts = 0
                self.first_success_attempt = attempts_to_fail + 1

            def run(self):
                self.attempts += 1
                if self.attempts >= self.first_success_attempt:
                    return self.attempts
                raise RuntimeError(self.attempts)

        self.assertEqual(
            1, retry_fn(Retriable().run, delays=[], what="succeeds immediately")
        )

        # Check log messages, and ensure that delays add up as expected
        start_time = time.time()
        with self.assertLogs(common_log) as log_ctx:
            self.assertEqual(
                4,
                retry_fn(
                    Retriable(3).run,
                    delays=[0, 0.1, 0.2],
                    what="succeeds on try 4",
                ),
            )
        self.assertTrue(
            any(
                "\n[Retry 3 of 3] succeeds on try 4 -- waiting 0.2 seconds.\n"
                in o
                for o in log_ctx.output
            )
        )
        self.assertGreater(time.time() - start_time, 0.3)

        # Check log to debug
        with self.assertLogs(common_log, level=logging.DEBUG) as log_ctx:
            self.assertEqual(
                4,
                retry_fn(
                    Retriable(3).run,
                    delays=[0, 0.1, 0.2],
                    what="succeeds on try 4",
                    log_exception=False,
                ),
            )
        self.assertTrue(
            any(
                "\n[Retry 3 of 3] succeeds on try 4 -- waiting 0.2 seconds.\n"
                in o
                for o in log_ctx.output
            )
        )

        # Check running out of retries
        with self.assertLogs(common_log) as log_ctx, self.assertRaises(
            RuntimeError
        ) as ex_ctx:
            retry_fn(Retriable(100).run, delays=[0] * 7, what="never succeeds")
        self.assertTrue(
            any(
                "\n[Retry 7 of 7] never succeeds -- waiting 0 seconds.\n" in o
                for o in log_ctx.output
            )
        )
        self.assertEqual((8,), ex_ctx.exception.args)

        # Test is_exception_retriable
        def _is_retryable(e):
            if isinstance(e, RuntimeError):
                return False
            return True

        with self.assertRaises(RuntimeError) as ex_ctx:
            retry_fn(
                Retriable(10).run,
                _is_retryable,
                delays=[0] * 5,
                what="never retries",
            )
        self.assertEqual((1,), ex_ctx.exception.args)

    def test_retryable(self) -> None:
        @retryable("got {a}, {b}, {c}", [0])
        def to_be_retried(a: int, b: int, c: int = 5):
            raise RuntimeError("retrying...")

        with self.assertRaises(RuntimeError), self.assertLogs(
            common_log
        ) as logs:
            to_be_retried(1, b=2)
        self.assertIn("got 1, 2, 5", "".join(logs.output))

    async def test_async_retry_fn(self) -> None:
        class Retriable:
            def __init__(self, attempts_to_fail=0):
                self.attempts = 0
                self.first_success_attempt = attempts_to_fail + 1

            async def run(self):
                self.attempts += 1
                if self.attempts >= self.first_success_attempt:
                    return self.attempts
                raise RuntimeError(self.attempts)

        self.assertEqual(
            1,
            await async_retry_fn(
                Retriable().run, delays=[], what="succeeds immediately"
            ),
        )

        # Check log messages, and ensure that delays add up as expected
        start_time = time.time()
        with self.assertLogs(common_log) as log_ctx:
            self.assertEqual(
                4,
                await async_retry_fn(
                    Retriable(3).run,
                    delays=[0, 0.1, 0.2],
                    what="succeeds on try 4",
                ),
            )
        self.assertTrue(
            any(
                "\n[Retry 3 of 3] succeeds on try 4 -- waiting 0.2 seconds.\n"
                in o
                for o in log_ctx.output
            )
        )
        self.assertGreater(time.time() - start_time, 0.3)

        # Check log to debug
        with self.assertLogs(common_log, level=logging.DEBUG) as log_ctx:
            self.assertEqual(
                4,
                await async_retry_fn(
                    Retriable(3).run,
                    delays=[0, 0.1, 0.2],
                    what="succeeds on try 4",
                    log_exception=False,
                ),
            )
        self.assertTrue(
            any(
                "\n[Retry 3 of 3] succeeds on try 4 -- waiting 0.2 seconds.\n"
                in o
                for o in log_ctx.output
            )
        )

        # Check running out of retries
        with self.assertLogs(common_log) as log_ctx, self.assertRaises(
            RuntimeError
        ) as ex_ctx:
            await async_retry_fn(
                Retriable(100).run, delays=[0] * 7, what="never succeeds"
            )
        self.assertTrue(
            any(
                "\n[Retry 7 of 7] never succeeds -- waiting 0 seconds.\n" in o
                for o in log_ctx.output
            )
        )
        self.assertEqual((8,), ex_ctx.exception.args)

        # Test is_exception_retriable
        def _is_retryable(e):
            if isinstance(e, RuntimeError):
                return False
            return True

        with self.assertRaises(RuntimeError) as ex_ctx:
            await async_retry_fn(
                Retriable(10).run,
                _is_retryable,
                delays=[0] * 5,
                what="never retries",
            )
        self.assertEqual((1,), ex_ctx.exception.args)

    async def test_async_retryable(self) -> None:
        @async_retryable("got {a}, {b}, {c}", [0])
        async def to_be_retried(a: int, b: int, c: int = 5):
            raise RuntimeError("retrying...")

        with self.assertRaises(RuntimeError), self.assertLogs(
            common_log
        ) as logs:
            await to_be_retried(1, b=2)
        self.assertIn("got 1, 2, 5", "".join(logs.output))

    def test_retryable_skip(self) -> None:
        iters = 0

        @retryable(
            "got {a}, {b}, {c}",
            [0, 0, 0],
            is_exception_retryable=lambda _: False,
        )
        def to_be_retried(a: int, b: int, c: int = 5):
            nonlocal iters
            iters += 1
            raise RuntimeError("retrying...")

        with self.assertRaises(RuntimeError):
            to_be_retried(1, b=2)
        self.assertEqual(1, iters)

    @unittest.mock.patch("antlir.common._mockable_platform_release")
    def test_kernel_version(self, platform_release) -> None:
        uname_to_tuples = {
            "5.2.9-129_fbk13_hardened_3948_ga3d2430737fa": (5, 2),
            "5.11.4-arch1-1": (5, 11),
            "4.16.9-old-busted": (4, 16),
        }

        for uname, parsed in uname_to_tuples.items():
            platform_release.return_value = uname

            self.assertEqual(kernel_version(), parsed)

    async def test_async_run(self) -> None:
        cmd: List[Union[bytes, str]] = ["echo", "-n", "hithere"]
        res = await async_run(cmd, check=True, stdout=asyncio.subprocess.PIPE)
        self.assertEqual(b"hithere", res.stdout)
        self.assertFalse(res.stderr)
        self.assertEqual(0, res.returncode)
        self.assertEqual(cmd, res.args)

    async def test_async_run_check_failure(self) -> None:
        with self.assertRaises(subprocess.CalledProcessError):
            await async_run(["sh", "exit", "1"], check=True)

    async def test_async_run_missing_pipe_stdin(self) -> None:
        with self.assertRaisesRegex(AssertionError, "You must set"):
            await async_run(["foo"], input=b"hello there")

    async def test_async_run_shell(self) -> None:
        cmd = "echo -n hithere"
        res = await async_run_shell(
            cmd, check=True, shell=True, stdout=asyncio.subprocess.PIPE
        )
        self.assertEqual(b"hithere", res.stdout)
        self.assertFalse(res.stderr)
        self.assertEqual(0, res.returncode)
        self.assertEqual(cmd, res.args)
