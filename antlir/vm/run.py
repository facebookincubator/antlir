#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import importlib.resources
import logging
import sys

import click
from antlir.vm.common import async_wrapper
from antlir.vm.vm import vm


logger = logging.getLogger(__file__)


@click.command()
@click.option("-v", "--verbose", count=True)
@click.option("--dry-run", is_flag=True, help="print qemu command and exit")
@click.option(
    "--timeout",
    type=int,
    help="seconds to wait for cmd to complete",
    default=60 * 60,
)
@click.argument("cmd", nargs=-1)
@async_wrapper
async def run(verbose, dry_run, timeout, cmd):
    # warn is 30, should default to 30 when verbose=0
    # each level below warning is 10 less than the previous
    log_level = -10 * verbose + 30
    logging.basicConfig(
        format="%(levelname)s:%(name)s: %(message)s", level=log_level
    )

    returncode = 0

    # if we didn't get a comamnd, use a shell
    cmd = cmd or ["/bin/bash"]

    with importlib.resources.path(__package__, "image") as image:
        async with vm(
            image=image,
            verbose=verbose > 0,
            interactive=not cmd or cmd == ["/bin/bash"],
            dry_run=dry_run,
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
