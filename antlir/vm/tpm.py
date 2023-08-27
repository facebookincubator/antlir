#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import os
import subprocess
import threading
from dataclasses import dataclass, field
from typing import Iterable, List, Union

from antlir.fs_utils import Path
from antlir.unshare import Unshare
from antlir.vm.common import SidecarProcess

logger = logging.getLogger(__name__)


class TPMError(Exception):
    pass


# NOTE: all the sidecars in vm.py use async api, while the QEMU process
# uses subprocess.Popen; since we want the TPM messages to be piped to stdout
# in real time (as opposed to next schedule point of the async loop), this
# software TPM process also needs to use subprocess.Popen along with an output
# reader thread that pipes to logger.
# The wait method here is marked async for the same reason.
class PseudoAsyncProcess(SidecarProcess):
    def __init__(self, args: List[Union[str, bytes]]):
        env = os.environ.copy()
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        proc = subprocess.Popen(
            args,
            preexec_fn=os.setpgrp,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        super().__init__(proc)

        self.__reader = threading.Thread(
            target=self.__pipe_output,
        )
        self.__reader.start()

    def __pipe_output(self):
        for line in self._proc.stderr:
            logger.debug("TPM: {}".format(line.strip()))

    async def wait(self):
        # other side of the read pipe is closed by the kill
        self.__reader.join()
        self._proc.stderr.close()

        self._proc.wait()


@dataclass(frozen=True)
class VmTPM:
    context_path: Path

    sock_path: Path = field(init=False)
    state_path: Path = field(init=False)

    def __post_init__(self):
        # sacrifices for a frozen instance with init fields, see:
        # https://docs.python.org/3/library/dataclasses.html#frozen-instances
        object.__setattr__(self, "sock_path", self.context_path / "tpm_ctrl.sock")
        object.__setattr__(self, "state_path", self.context_path / "tpm_state")
        os.mkdir(self.state_path)

    async def start_sidecar(
        self,
        ns: Unshare,
        binary: str,
        timeout_ms: int,
    ) -> PseudoAsyncProcess:
        logger.debug(f"Starting software TPM... [context: {self.context_path}]")
        proc = PseudoAsyncProcess(
            ns.nsenter_as_user(binary, *self.__get_sidecar_args())
        )

        try:
            self.sock_path.wait_for(timeout_ms=timeout_ms)
        except FileNotFoundError:
            await proc.kill()
            raise TPMError(
                f"Software TPM device fail to create socket in: {timeout_ms}ms"
            )

        logger.debug(f"TPM sidecar PID: {proc.pid}")
        return proc

    def __get_sidecar_args(self) -> Iterable[str]:
        args = [
            "socket",
            "--tpm2",
            "--tpmstate",
            f"dir={self.state_path}",
            "--ctrl",
            f"type=unixio,path={self.sock_path}",
        ]

        if 0 < logger.getEffectiveLevel() <= logging.DEBUG:
            # this level 20 is basically binary, anything >= 5 generates output
            args.extend(
                [
                    "--log",
                    "level=20",
                ]
            )

        return args

    @property
    def qemu_args(self) -> Iterable[str]:
        return (
            "-chardev",
            f"socket,id=chtpm,path={self.sock_path}",
            "-tpmdev",
            "emulator,id=tpm0,chardev=chtpm",
            "-device",
            "tpm-tis,tpmdev=tpm0",
        )
