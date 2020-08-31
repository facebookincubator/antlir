#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import importlib.resources
import io
import logging
import os.path
import sys
import time
from functools import wraps
from typing import Iterable, List, Optional

import click
from fs_image.artifacts_dir import find_repo_root
from fs_image.vm.vm import kernel_vm


logger = logging.getLogger("vmtest")


MINUTE = 60


def async_command(f):
    @wraps(f)
    def wrapper(*args, **kwargs):
        loop = asyncio.get_event_loop()
        return loop.run_until_complete(f(*args, **kwargs))

    return wrapper


class RelativeTimeFormatter(logging.Formatter):
    def format(self, record):
        # this is technically "uptime" from when the logger initializes, but
        # the vm should be started within milliseconds so it's "good enough"
        # for relative ordering between python logs and guest kernel logs
        record.uptime = record.relativeCreated / 1000.0
        return super().format(record)


def blocking_print(*args, file: io.IOBase = sys.stdout, **kwargs):
    blocking = os.get_blocking(file.fileno())
    os.set_blocking(file.fileno(), True)
    print(*args, file=file, **kwargs)
    # reset to the old blocking mode
    os.set_blocking(file.fileno(), blocking)


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
    default=5 * MINUTE,
    help="how many seconds to wait for the test to finish",
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
    setenv: List[str],
    test_type: str,
    sync_file: List[str],
    gtest_list_tests: bool,
    list_tests: Optional[str],
    interactive: bool,
    ncpus: int,
    args: Iterable[str],
) -> None:
    h = logging.StreamHandler()
    h.setFormatter(
        RelativeTimeFormatter(
            "%(uptime).03f %(levelname)s:%(name)s: %(message)s"
        )
    )
    logging.basicConfig(level=logging.DEBUG, handlers=[h])
    returncode = -1
    start_time = time.time()
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
            ncpus=ncpus,
        ) as vm:
            boot_time_elapsed = time.time() - start_time
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
                cmd = ["/test"] + list(args)
                logger.debug(f"executing {cmd} inside guest")
                returncode, stdout, stderr = await vm.run(
                    cmd=cmd,
                    # a certain amount of the total timeout is allocated for
                    # the host to boot, subtract the amount of time it actually
                    # took, so that vmtest times out internally before choking
                    # to TestPilot, which gives the same end result but should
                    # allow for some slightly better logging opportunities
                    timeout=timeout - boot_time_elapsed - 1,
                    env=test_env,
                    cwd=fbcode,
                )
                # Some tests have incredibly large amounts of output, which
                # results in a BlockingIOError when stdout/err are in
                # non-blocking mode. Just force it to print the output in
                # blocking mode to avoid that - we don't really care how long
                # it ends up blocked as long as it eventually gets written.
                blocking_print(stdout.decode("utf-8"), end="")
                blocking_print(stderr.decode("utf-8"), file=sys.stderr, end="")

                for path in file_arguments:
                    logger.debug(f"copying {path} back to the host")
                    # copy any files that were written in the guest back to the
                    # host so that TestPilot can read from where it expects
                    # outputs to end up
                    outfile_contents = await vm.cat_file(str(path))
                    with open(path, "wb") as out:
                        out.write(outfile_contents)

    sys.exit(returncode)


if __name__ == "__main__":
    main()
