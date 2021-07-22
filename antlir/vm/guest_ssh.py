#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib
import os
import subprocess
import tempfile
from dataclasses import dataclass
from itertools import chain
from typing import Iterable, List, Mapping, Optional, Tuple, cast

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
    # pyre-fixme[8]: Attribute has type `Mapping[str, str]`; used as `None`.
    options: Mapping[str, str] = None
    # pyre-fixme[8]: Attribute has type `Path`; used as `None`.
    privkey: Path = None

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

    async def run(
        self,
        cmd: Iterable[str],
        timeout_ms: int,
        env: Optional[Mapping[str, str]] = None,
        check: bool = False,
        cwd: Optional[Path] = None,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) -> Tuple[int, bytes, bytes]:
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

        cmd = self.ssh_cmd(timeout_ms=timeout_ms) + ["--"] + cmd_pre + cmd

        logger.debug(f"Running {cmd} in vm at {self.tapdev.guest_ipv6_ll}")
        res = subprocess.run(
            cmd,
            check=check,
            stdout=stdout,
            stderr=stderr,
            # Future: handle stdin properly so that we can pipe input from
            # the caller into a program being executing inside a VM
            timeout=timeout_ms / 1000,
        )
        logger.debug(f"res: {res.returncode}, {res.stdout}, {res.stderr}")
        return res.returncode, res.stdout, res.stderr

    def ssh_cmd(self, timeout_ms: int, **kwargs) -> List[str]:
        options = {
            # just ignore the ephemeral vm fingerprint
            "UserKnownHostsFile": "/dev/null",
            "StrictHostKeyChecking": "no",
            "ConnectTimeout": int(timeout_ms / 1000),
            "ConnectionAttempts": 10,
        }

        if self.options:
            logger.debug(f"Additional options: {self.options}")
            options.update(self.options)

        options = list(
            chain.from_iterable(["-o", f"{k}={v}"] for k, v in options.items())
        )
        return cast(
            List[str],
            self.tapdev.netns.nsenter_as_user(
                "ssh",
                *options,
                "-i",
                str(self.privkey),
                f"root@{self.tapdev.guest_ipv6_ll}",
            ),
        )
