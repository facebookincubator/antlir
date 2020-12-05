#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import base64
import json
import os
import random
import sys
from contextlib import asynccontextmanager
from dataclasses import dataclass
from datetime import timedelta
from typing import (
    Any,
    AsyncContextManager,
    Dict,
    Iterable,
    Mapping,
    Optional,
    Tuple,
)

from antlir.common import async_retryable


class QemuError(Exception):
    pass


DEFAULT_CONNECT_TIMEOUT = timedelta(seconds=1)
DEFAULT_EXEC_TIMEOUT = timedelta(seconds=60)
STREAM_LIMIT = 2 ** 20  # 1 MB


@dataclass(frozen=True)
class QemuGuestAgent(object):

    path: os.PathLike
    connect_timeout: int = DEFAULT_CONNECT_TIMEOUT

    @async_retryable("Failed to find {self.path}", [0.1] * 5)
    async def _open(self) -> Tuple[asyncio.StreamReader, asyncio.StreamWriter]:
        return await asyncio.open_unix_connection(self.path, limit=STREAM_LIMIT)

    @asynccontextmanager
    async def _connect(
        self,
    ) -> AsyncContextManager[Tuple[asyncio.StreamReader, asyncio.StreamWriter]]:

        # Qemu creates the socket file for us, sometimes it can be a bit slow
        # and we will try and connect before it is created. `_open()` will
        # retry until the file shows up.
        r, w = await self._open()

        try:
            sync_id = random.randint(0, sys.maxsize)
            req = {
                "execute": "guest-sync-delimited",
                "arguments": {"id": sync_id},
            }
            w.write(b"\xFF")
            w.write(json.dumps(req).encode("utf-8"))
            # that can wait until it becomes necessary, right now things seem to
            # be generally working
            await w.drain()
            await r.readuntil(b"\xFF")
            resp = json.loads(await r.readline())
            if resp["return"] != sync_id:
                raise QemuError(
                    f"guest-sync-delimited ID does not match {sync_id}: {resp}"
                )
            yield r, w
        except ConnectionResetError as err:
            raise QemuError("Guest agent connection reset") from err
        finally:
            if not w.is_closing():
                w.close()
                await w.wait_closed()

    async def _call(
        self,
        call: Dict[str, Any],
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> Dict[str, Any]:
        writer.write(json.dumps(call).encode("utf-8"))
        await writer.drain()
        received = await reader.readline()
        if reader.at_eof():
            raise QemuError("Reached EOF")
        res = json.loads(received)
        if "error" in res:
            raise QemuError(res["error"])
        return res["return"]

    async def exec_sync(self, *args, **kwargs) -> Tuple[int, str, str]:
        return await self.run(*args, **kwargs)

    async def run(
        self,
        cmd: Iterable[str],
        timeout: timedelta = DEFAULT_EXEC_TIMEOUT,
        env: Optional[Mapping[str, str]] = None,
        cwd: Optional[os.PathLike] = None,
    ) -> Tuple[int, bytes, bytes]:
        """run a command inside the vm and optionally pipe stdout/stderr to the
        parent
        """
        async with self._connect() as (r, w):
            cmd = list(cmd)
            path = cmd[0]
            args = cmd[1:]
            env = env or {}
            if isinstance(timeout, timedelta):
                timeout = timeout.seconds
            systemd_run_args = [
                "--pipe",
                "--wait",
                "--quiet",
                "--service-type=exec",
                f"--property=RuntimeMaxSec={str(timeout)}",
            ]
            systemd_run_args += [
                f"--setenv={key}={val}" for key, val in env.items()
            ]
            if cwd is not None:
                systemd_run_args += [f"--working-directory={str(cwd)}"]
            pid = await self._call(
                {
                    "execute": "guest-exec",
                    "arguments": {
                        "path": "/bin/systemd-run",
                        "arg": systemd_run_args + ["--", str(path)] + args,
                        "capture-output": True,
                    },
                },
                r,
                w,
            )
            pid = pid["pid"]
            while True:
                status = await self._call(
                    {"execute": "guest-exec-status", "arguments": {"pid": pid}},
                    r,
                    w,
                )
                # output is only made available when the process exits
                if status["exited"]:
                    stdout = base64.b64decode(status.get("out-data", b""))
                    stderr = base64.b64decode(status.get("err-data", b""))
                    return status["exitcode"], stdout, stderr
                await asyncio.sleep(0.1)

    async def cat_file(self, path: os.PathLike) -> bytes:
        async with self._connect() as (r, w):
            handle = await self._call(
                {
                    "execute": "guest-file-open",
                    "arguments": {"path": str(path)},
                },
                r,
                w,
            )
            contents = b""
            while True:
                read = await self._call(
                    {
                        "execute": "guest-file-read",
                        "arguments": {"handle": handle},
                    },
                    r,
                    w,
                )
                contents += base64.b64decode(read["buf-b64"])
                if read["eof"]:
                    return contents
