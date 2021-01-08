#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import sys
from typing import (
    Iterable,
    List,
)

from antlir.common import get_logger
from antlir.vm.vm import vm, VMExecOpts
from antlir.vm.vm_opts_t import vm_opts_t


log = get_logger()


class VMRunExecOpts(VMExecOpts):
    cmd: List[str] = ["/bin/bash"]

    @classmethod
    def setup_cli(cls, parser):
        super(VMRunExecOpts, cls).setup_cli(parser)

        parser.add_argument(
            "cmd",
            nargs="*",
            default=["/bin/bash"],
            help="The command to run inside the VM",
        )


async def run(
    # common args from VMExecOpts
    bind_repo_ro: bool,
    debug: bool,
    extra: List[str],
    opts: vm_opts_t,
    timeout_ms: int,
    # antlir.vm.run specific args
    cmd: List[str],
):
    async with vm(
        bind_repo_ro=bind_repo_ro,
        opts=opts,
        verbose=debug,
        timeout_ms=timeout_ms,
        interactive=cmd == ["/bin/bash"],
    ) as (instance, _, _):
        returncode, _, _ = await instance.run(cmd)

    sys.exit(returncode)


if __name__ == "__main__":
    asyncio.run(run(**dict(VMRunExecOpts.parse_cli(sys.argv[1:]))))
