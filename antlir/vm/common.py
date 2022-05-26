# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import contextlib
import logging
import os
import subprocess
from functools import wraps
from typing import Awaitable

logger = logging.getLogger(__name__)


def async_wrapper(f):
    """Decorate a function to run in an async event loop."""

    @wraps(f)
    def wrapper(*args, **kwargs):
        loop = asyncio.get_event_loop()
        return loop.run_until_complete(f(*args, **kwargs))

    return wrapper


def insertstack(f):
    """
    Decorate an `asynccontextmanager` to insert an `AsyncExitStack` that it can
    use internally.  The `AsyncExitStack` is passed to the wrapped function via
    the `stack=` kwarg
    """

    # TODO: maybe inspect f to make sure it is really an asynccontextmanager?
    @wraps(f)
    async def wrapper(*args, **kwargs):
        async with contextlib.AsyncExitStack() as stack:
            async with f(*args, stack=stack, **kwargs) as r:
                yield r

    return contextlib.asynccontextmanager(wrapper)


class SidecarProcess:
    """
    Encapsulated class for sidecar processes that are using the async
    stack and can spawn their own children.
    Unless requiring specific customization, should only be created
    by the create_sidecar_subprocess() func
    """

    def __init__(self, proc):
        # asyncio.Process is not exported, so can't type the proc here
        self._proc = proc

    async def kill(self):
        subprocess.run(
            [
                "sudo",
                "kill",
                "-KILL",
                "--",
                "-{}".format(self.pid),
            ]
        )

        # dont leak resources
        await self.wait()
        logger.debug(f"Killed sidecar, pid: {self.pid}")

    async def wait(self):
        await self._proc.wait()

    @property
    def pid(self):
        return self._proc.pid


async def create_sidecar_subprocess(
    program: str, *args, stdin=None, stdout=None, stderr=None, **kwargs
) -> SidecarProcess:
    env = os.environ.copy()
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    # NOTE(aeh): in order to end all the process tree for the sidecars,
    # the exec below sets each one as a process group leader; the kill
    # then sends the signal to all of the children that the sidecar
    # process might have spawned
    proc = await asyncio.create_subprocess_exec(
        program,
        preexec_fn=os.setpgrp,
        *args,
        stdin=stdin,
        stdout=stdout,
        stderr=stderr,
        env=env,
        **kwargs,
    )

    logger.debug(f"Started sidecar, pid: {proc.pid}")
    return SidecarProcess(proc)
