#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import asyncio
import os
import sys

from antlir.common import get_logger, init_logging, pipe
from antlir.fs_utils import Path

log = get_logger()


async def _pump(r, w) -> None:
    while True:
        data = r.read(4096)
        if not data:
            break
        w.write(data)
        await w.drain()

    w.close()
    await w.wait_closed()


def rewrite_testpilot_python_cmd(cmd, env, fd):
    log.debug(f"Rewrite python cmd: {cmd}, {env}, {fd}")
    env["TEST_PILOT"] = "True"
    return [
        cmd[0],
        "--output",
        f"/proc/self/fd/{fd}",
        *cmd[1:],
    ], env


def rewrite_tpx_gtest_cmd(cmd, env, fd):
    log.debug(f"Rewrite gtest cmd: {cmd}, {env}, {fd}")
    env["GTEST_OUTPUT"] = f"xml:/proc/self/fd/{fd}"
    return [*cmd], env


_TEST_TYPE_TO_REWRITE_CMD = {
    "pyunit": rewrite_testpilot_python_cmd,
    "gtest": rewrite_tpx_gtest_cmd,
}


async def main(argv) -> int:
    init_logging(debug=True)

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--test-type",
        type=str,
        choices=_TEST_TYPE_TO_REWRITE_CMD.keys(),
        help="The type of test that is being wrapped.",
    )
    parser.add_argument(
        "--socket",
        type=Path.from_argparse,
        required=False,
        help="The socket path where output from the test should be sent",
    )
    parser.add_argument(
        "cmd",
        nargs="+",
    )
    opts = parser.parse_args(argv)

    rewrite_cmd = _TEST_TYPE_TO_REWRITE_CMD[opts.test_type]

    with pipe() as (read_pipe, write_pipe):
        _, writer = await asyncio.open_unix_connection(opts.socket)

        cmd, env = rewrite_cmd(opts.cmd, os.environ.copy(), write_pipe.fileno())

        log.debug(f"rewritten cmd: {cmd}")
        log.debug(f"env: {os.environ}")
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            env=env,
            pass_fds=[write_pipe.fileno()],
        )
        # proc fully owns this now
        write_pipe.close()

        # Wait
        await asyncio.gather(
            proc.wait(),
            _pump(read_pipe, writer),
        )

    log.debug(f"proc.returncode: {proc.returncode}")
    return proc.returncode or -1


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1:]))
