#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import sys
from typing import Iterable

import click
from antlir.common import init_logging, get_logger
from antlir.vm.common import async_wrapper
from antlir.vm.vm import vm
from antlir.vm.vm_opts_t import vm_opts_t


logger = get_logger()


@click.command()
@click.option(
    "--opts",
    type=vm_opts_t.parse_raw,
    help="Path to a serialized vm_opts_t instance containing configuration "
    "details for the vm.",
    required=True,
)
@click.option(
    "--bind-repo-ro",
    is_flag=True,
    help="Makes a read-only bind-mount of the current Buck "
    "project into the vm at the same location as it is on "
    "the host. This is needed to run binaries that are built "
    "to be run in-place.",
)
@click.option("-d", "--debug", is_flag=True, default=False)
@click.option("-v", "--verbose", count=True)
@click.option(
    "--timeout",
    type=int,
    help="seconds to wait for cmd to complete",
    default=60 * 60,
)
@click.argument("cmd", nargs=-1)
@async_wrapper
async def run(
    cmd: Iterable[str],
    bind_repo_ro: bool,
    debug: bool,
    opts: vm_opts_t,
    timeout: int,
    verbose: int,
):
    init_logging(debug=debug)
    returncode = 0

    # if we didn't get a comamnd, use a shell
    cmd = cmd or ["/bin/bash"]

    async with vm(
        opts=opts,
        bind_repo_ro=bind_repo_ro,
        verbose=verbose > 0,
        interactive=not cmd or cmd == ["/bin/bash"],
    ) as instance:
        if cmd:
            try:
                returncode, _, _ = await instance.run(cmd, timeout=timeout)
            except asyncio.TimeoutError:
                click.echo(f"'{' '.join(cmd)}' timed out!", err=True)
                sys.exit(124)

    sys.exit(returncode)


if __name__ == "__main__":
    run()
