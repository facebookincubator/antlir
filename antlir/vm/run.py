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
    Optional,
)

from antlir.vm.vm import ConsoleRedirect, ShellMode, vm, VMExecOpts
from antlir.vm.vm_opts_t import vm_opts_t


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
):
    # This is just a shortcut so that if the user doesn't provide a command
    # we drop them into a shell using the standard mechanism for that.
    if not cmd and not shell:
        shell = ShellMode.ssh

    returncode = 0
    async with vm(
        bind_repo_ro=bind_repo_ro,
        opts=opts,
        console=console,
        timeout_ms=timeout_ms,
        shell=shell,
    ) as (instance, boot_ms, timeout_ms):
        # If we are run with `--shell` mode, we don't get an instance since
        # the --shell mode takes over.  This is a bit of a wart that exists
        # because if a context manager doesn't yield *something* it will
        # throw an exception that this caller has to handle.
        if instance:
            returncode, stdout, stderr = await instance.run(
                cmd, timeout_ms=timeout_ms
            )

            # We want to write whatever we get from the command out to the
            # respective fds.
            # Note: in the near future this will be replaced with ssh,
            # which can be setup to just write directly to the users
            # stdout/stderr fd's instead of having to buffer like this.
            sys.stdout.write(stdout.decode())
            sys.stderr.write(stderr.decode())

    sys.exit(returncode)


if __name__ == "__main__":
    asyncio.run(run(**dict(VMRunExecOpts.parse_cli(sys.argv[1:]))))
