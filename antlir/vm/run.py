#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import sys
from typing import List, Optional

from antlir.common import get_logger
from antlir.vm.bzl.vm import vm_opts_t
from antlir.vm.vm import ConsoleRedirect, ShellMode, vm, VMExecOpts


log = get_logger()


class VMRunExecOpts(VMExecOpts):
    cmd: Optional[List[str]] = None

    @classmethod
    def setup_cli(cls, parser):
        super(VMRunExecOpts, cls).setup_cli(parser)

        parser.add_argument(
            "cmd",
            nargs="*",
            help="The command to run inside the VM.  If no command is provided "
            "the user will be dropped into a shell using the ShellMode.",
        )


async def run(
    # common args from VMExecOpts
    bind_repo_ro: bool,
    console: ConsoleRedirect,
    debug: bool,
    extra: List[str],
    opts: vm_opts_t,
    shell: Optional[ShellMode],
    timeout_ms: int,
    # antlir.vm.run specific args
    cmd: List[str],
) -> Optional[int]:
    # This is just a shortcut so that if the user doesn't provide a command
    # we drop them into a shell using the standard mechanism for that.
    if not cmd and not shell:
        shell = ShellMode.ssh

    returncode = 0
    async with vm(
        bind_repo_ro=bind_repo_ro,
        opts=opts.copy(
            update={
                "append": (
                    *opts.append,
                    # See the note in `nspawn.py` about why we have to set
                    # this on the kernel command line -- otherwise it would
                    # not be passed to all the units.  We also have to set
                    # it below via `env`.
                    (
                        "systemd.setenv="
                        "ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP=1"
                    ),
                ),
            }
        ),
        console=console,
        timeout_ms=timeout_ms,
        shell=shell,
    ) as (instance, boot_ms, timeout_ms):

        # If we are run with `--shell` mode, we don't get an instance since
        # the --shell mode takes over.  This is a bit of a wart that exists
        # because if a context manager doesn't yield *something* it will
        # throw an exception that this caller has to handle.
        if instance:
            res = await instance.run(
                cmd, stderr=None, stdout=None, timeout_ms=timeout_ms
            )
            log.info(f"{cmd} completed with: {res.returncode}")
            returncode = res.returncode

    return returncode


def main() -> None:
    asyncio.run(run(**dict(VMRunExecOpts.parse_cli(sys.argv[1:]))))


if __name__ == "__main__":
    main()
