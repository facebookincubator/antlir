#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import importlib.resources
import os.path
import sys
from functools import wraps
from pathlib import Path
from typing import Iterable, List, Optional

import click
from fs_image.artifacts_dir import find_repo_root
from fs_image.vm.vm import Share, kernel_vm


MINUTE = 60


def async_command(f):
    @wraps(f)
    def wrapper(*args, **kwargs):
        loop = asyncio.get_event_loop()
        return loop.run_until_complete(f(*args, **kwargs))

    return wrapper


@click.command(context_settings={"ignore_unknown_options": True})
@click.option(
    "-q/-e",
    "--quiet/--echo",
    default=False,
    help="hide all vm output (including boot messages)",
)
@click.option(
    "--timeout",
    type=int,
    # TestPilot sets this environment variable
    envvar="TIMEOUT",
    default=10 * MINUTE,
    help="how many seconds to wait for the test to finish",
)
@click.option(
    "--up-timeout",
    type=int,
    default=10 * MINUTE,
    help="how many seconds to wait for vm to boot",
)
@click.option(
    "--setenv",
    type=str,
    multiple=True,
    help="Specify an environment variable assignment of form NAME=VALUE",
)
@click.option(
    "--test-type",
    type=click.Choice(["gtest", "pyunit"]),
    help="Which test type we are pretending to be",
    required=False,
)
@click.option(
    "--sync-file",
    type=str,
    help="Sync this file for tpx from the vm to the host.",
    multiple=True,
)
@click.option("--gtest_list_tests", is_flag=True)  # C++ gtest
@click.option("--list-tests")  # Python pyunit with the new TestPilot adapter
@click.option(
    "--interactive", is_flag=True, help="Connect VM console in foreground"
)
@click.option(
    "--ncpus", type=int, default=1, help="How many vCPUs the VM will have."
)
@click.argument("args", nargs=-1, type=click.UNPROCESSED)
@async_command
async def main(
    quiet: bool,
    timeout: int,
    up_timeout: int,
    setenv: List[str],
    test_type: str,
    sync_file: List[str],
    gtest_list_tests: bool,
    list_tests: Optional[str],
    interactive: bool,
    ncpus: int,
    args: Iterable[str],
) -> None:
    returncode = -1
    fbcode = find_repo_root()
    test_env = dict(s.split("=", maxsplit=1) for s in setenv)

    with importlib.resources.path(__package__, "image") as image:
        if gtest_list_tests or list_tests:
            assert not (gtest_list_tests and list_tests), sys.argv
            # Start the test binary directly to list out test cases instead of
            # starting an entire VM.  This is faster, but it's also a potential
            # security hazard since the test code may expect that it always runs
            # sandboxed, and may run untrusted code as part of listing tests.
            # TODO(vmagro): the long-term goal should be to make vm boots as
            # fast as possible to avoid unintuitive tricks like this
            with importlib.resources.path(
                "fs_image.vm", "test_discovery_binary"
            ) as inner_test_on_host:
                proc = await asyncio.create_subprocess_exec(
                    str(inner_test_on_host),
                    *(
                        ["--gtest_list_tests"]
                        if gtest_list_tests
                        # NB: Unlike for the VM, we don't explicitly have to
                        # pass the magic `TEST_PILOT` environment var to allow
                        # triggering the new TestPilotAdapter. The environment
                        # is inherited.
                        else ["--list-tests", list_tests]
                    ),
                )
                await proc.wait()
            sys.exit(proc.returncode)

        async with kernel_vm(
            image=image,
            fbcode=fbcode,
            verbose=not quiet,
            interactive=interactive,
            up_timeout=up_timeout,
            ncpus=ncpus,
        ) as vm:
            if not interactive:
                # Automatically execute test only under non-interactive mode.

                # Sync the file which tpx needs from the vm to the host.
                file_arguments = list(sync_file)

                for arg in args:
                    # for any args that look like files make sure that the
                    # directory exists so that the test binary can write to
                    # files that it expects to exist (that would normally be
                    # created by TestPilot)
                    dirname = os.path.dirname(arg)
                    # TestPilot will already create the directories on the
                    # host, so as another sanity check only create the
                    # directories in the VM that already exist on the host
                    if dirname and os.path.exists(dirname):
                        await vm.exec_sync(("mkdir", "-p", dirname))
                        file_arguments.append(arg)

                # The behavior of the FB-internal Python test main changes
                # completely depending on whether this environment var is set.
                # We must forward it so that the new TP adapter can work.
                test_pilot_env = os.environ.get("TEST_PILOT")
                if test_pilot_env:
                    test_env["TEST_PILOT"] = test_pilot_env
                returncode, _, _ = await vm.run(
                    cmd=["/test"] + list(args),
                    timeout=timeout,
                    env=test_env,
                    cwd=fbcode,
                )

                for path in file_arguments:
                    # copy any files that were written in the guest back to the
                    # host so that TestPilot can read from where it expects
                    # outputs to end up
                    outfile_contents = await vm.cat_file(str(path))
                    with open(path, "wb") as out:
                        out.write(outfile_contents)

    sys.exit(returncode)


if __name__ == "__main__":
    main()
