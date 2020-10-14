#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
from dataclasses import dataclass
from itertools import chain
from typing import Iterable, List, Mapping, Optional, Tuple

from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.common import DEFAULT_PATH_ENV
from antlir.vm.tap import VmTap


DEFAULT_CONNECT_TIMEOUT = 5
DEFAULT_EXEC_TIMEOUT = 60
STREAM_LIMIT = 2 ** 20  # 1 MB

DEFAULT_ENV = {"PATH": DEFAULT_PATH_ENV}


@dataclass(frozen=True)
class QemuGuestConnection(object):
    tapdev: VmTap
    ssh_privkey: Path
    connect_timeout: int = DEFAULT_CONNECT_TIMEOUT

    def run(
        self,
        cmd: Iterable[str],
        timeout: int = DEFAULT_EXEC_TIMEOUT,
        env: Optional[Mapping[str, str]] = None,
        cwd: Optional[Path] = None,
        check: bool = False,
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
            f"--property=RuntimeMaxSec={str(int(timeout))}",
        ] + [f"--setenv={key}={val}" for key, val in run_env.items()]
        if cwd is not None:
            systemd_run_args += [f"--working-directory={str(cwd)}"]

        cmd = (
            self.ssh_cmd(ConnectTimeout=self.connect_timeout)
            + systemd_run_args
            + ["--"]
            + cmd
        )
        res = subprocess.run(
            cmd,
            check=check,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            stdin=subprocess.DEVNULL,
            timeout=timeout,
        )
        return res.returncode, res.stdout, res.stderr

    def ssh_cmd(self, **kwargs: str) -> List[str]:
        options = {
            # just ignore the ephemeral vm fingerprint
            "UserKnownHostsFile": "/dev/null",
            "StrictHostKeyChecking": "no",
        }
        options.update(kwargs)
        options = list(
            chain.from_iterable(["-o", f"{k}={v}"] for k, v in options.items())
        )
        return self.tapdev.netns.nsenter_as_user(
            "ssh",
            *options,
            "-i",
            self.ssh_privkey,
            f"root@{self.tapdev.guest_ipv6_ll}",
            "--",
        )

    def cat_file(self, path: os.PathLike) -> bytes:
        _, stdout, _ = self.run(["cat", str(path)], check=True)
        return stdout
