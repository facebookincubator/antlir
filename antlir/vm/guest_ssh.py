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
from typing import Iterable, List, Mapping, Optional, Tuple

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
    privkey: Path = None
    timeout_sec: int = DEFAULT_TIMEOUT_SEC

    def __init__(self, tapdev: VmTap, timeout_sec: int = DEFAULT_TIMEOUT_SEC):
        self.tapdev = tapdev
        self.timeout_sec = timeout_sec

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
        except Exception as e:
            logger.error(f"Error removing privkey: {self.privkey}: {e}")
            pass

    async def run(
        self,
        cmd: Iterable[str],
        env: Optional[Mapping[str, str]] = None,
        cwd: Optional[Path] = None,
        check: bool = False,
        stdout=None,
        stderr=None,
    ) -> Tuple[int, bytes, bytes]:
        """run a command inside the vm and optionally pipe stdout/stderr to the
        parent
        """
        cmd = list(cmd)
        run_env = DEFAULT_ENV.copy()
        run_env.update(env or {})
        systemd_run_args = [
            "systemd-run",
            "--pipe",
            "--wait",
            "--quiet",
            "--service-type=exec",
        ] + [f"--setenv={key}={val}" for key, val in run_env.items()]

        if cwd is not None:
            systemd_run_args += [f"--working-directory={str(cwd)}"]

        cmd = (self.ssh_cmd() + ["--"], +systemd_run_args + ["--"] + cmd)

        logger.debug(f"Running {cmd} in vm at {self.tapdev.guest_ipv6_ll}")
        logger.debug(f"{' '.join(cmd)}")
        res = subprocess.run(
            cmd,
            check=check,
            stdout=stdout,
            stderr=stderr,
            # Never connect stdin
            stdin=subprocess.DEVNULL,
        )
        return res.returncode, res.stdout, res.stderr

    def ssh_cmd(self, **kwargs) -> List[str]:
        options = {
            # just ignore the ephemeral vm fingerprint
            "UserKnownHostsFile": "/dev/null",
            "StrictHostKeyChecking": "no",
            "ConnectTimeout": self.timeout_sec,
        }
        logger.debug(f"Additional options: {kwargs}")
        options.update(kwargs)
        options = list(
            chain.from_iterable(["-o", f"{k}={v}"] for k, v in options.items())
        )
        return self.tapdev.netns.nsenter_as_user(
            "ssh",
            *options,
            "-i",
            str(self.privkey),
            f"root@{self.tapdev.guest_ipv6_ll}",
        )

    def cat_file(self, path: os.PathLike) -> bytes:
        _, stdout, _ = self.run(["cat", str(path)], check=True)
        return stdout
