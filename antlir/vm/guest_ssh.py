#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import importlib
import os
import subprocess
import tempfile
from dataclasses import dataclass
from itertools import chain
from typing import Iterable, List, Mapping, Optional, Union

from antlir.common import get_logger
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.common import DEFAULT_PATH_ENV
from antlir.vm.tap import VmTap


DEFAULT_TIMEOUT_SEC = 60

DEFAULT_ENV = {"PATH": DEFAULT_PATH_ENV}

logger = get_logger()


@dataclass()
class GuestSSHConnection:
    tapdev: VmTap
    options: Mapping[str, Union[str, int]]
    privkey: Optional[Path] = None

    def __enter__(self):
        self.privkey = Path(tempfile.NamedTemporaryFile(delete=False).name)
        logger.debug(f"Enter ssh context. Load private key: {self.privkey}")
        with self.privkey.open(mode="w") as f:
            f.write(importlib.resources.read_text(__package__, "privkey"))
            f.flush()
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        logger.debug(f"Exit ssh context.  Remove private key: {self.privkey}")
        try:
            os.remove(self.privkey)
        except Exception as e:  # pragma: no cover
            logger.error(f"Error removing privkey: {self.privkey}: {e}")

    def ssh_cmd(
        self, *, timeout_ms: int, forward: Optional[Mapping[Path, Path]] = None
    ) -> List[Union[str, bytes]]:
        options = {
            # just ignore the ephemeral vm fingerprint
            "UserKnownHostsFile": "/dev/null",
            "StrictHostKeyChecking": "no",
            "ConnectTimeout": int(timeout_ms / 1000),
            "ConnectionAttempts": 10,
            "StreamLocalBindUnlink": "yes",
        }

        if self.options:
            logger.debug(f"Additional options: {self.options}")
            options.update(self.options)

        options = list(
            chain.from_iterable(["-o", f"{k}={v}"] for k, v in options.items())
        )

        maybe_forward = (
            list(chain.from_iterable(["-R", f"{k}:{v}"] for k, v in forward.items()))
            if forward
            else []
        )

        return self.tapdev.netns.nsenter_as_user(
            "ssh",
            *options,
            *maybe_forward,
            "-i",
            str(self.privkey),
            f"root@{self.tapdev.guest_ipv6_ll}",
        )

    async def run(
        self,
        cmd: Iterable[str],
        timeout_ms: int,
        check: bool = False,
        cwd: Optional[Path] = None,
        env: Optional[Mapping[str, str]] = None,
        forward: Optional[Mapping[Path, Path]] = None,
        stdout=None,
        stderr=None,
    ) -> asyncio.subprocess.Process:
        """
        run a command inside the vm
        """
        cmd = list(cmd)
        run_env = DEFAULT_ENV.copy()
        run_env.update(env or {})

        cmd_pre = []
        if cwd is not None:
            cmd_pre.append(f"cd {str(cwd)};")

        cmd_pre.append(
            " ".join(f"{key}={val}" for key, val in run_env.items()),
        )

        cmd = [
            *self.ssh_cmd(
                timeout_ms=timeout_ms,
                forward=forward,
            ),
            "--",
            *cmd_pre,
            *cmd,
        ]

        logger.debug(f"Running {cmd} in vm at {self.tapdev.guest_ipv6_ll}")
        logger.debug(f"{' '.join([str(c) for c in cmd])}")
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=stdout,
            stderr=stderr,
        )
        await proc.wait()
        if check and proc.returncode != 0:
            stdout, stderr = await proc.communicate()
            raise subprocess.CalledProcessError(
                returncode=proc.returncode or -1,
                cmd=cmd,
                stderr=stderr,
                output=stdout,
            )

        return proc
