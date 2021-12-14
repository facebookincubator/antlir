#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from dataclasses import dataclass
from typing import Iterable

from antlir.fs_utils import Path


@dataclass(frozen=True)
class VmTPM(object):
    sock_path: Path
    tpm_state_path: Path

    @property
    def sidecar_args(self) -> Iterable[str]:
        # TODO: pipe log
        return (
            "socket",
            "--tpm2",
            "--tpmstate",
            f"dir={self.tpm_state_path}",
            "--ctrl",
            f"type=unixio,path={self.sock_path}",
            # "--log",
            # "level=20",
        )

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
