#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import asyncio
import io
import os.path
import sys
import time
from typing import List, Optional

from antlir.artifacts_dir import find_buck_cell_root
from antlir.common import get_logger
from antlir.fs_utils import Path
from antlir.vm.share import BtrfsDisk, Plan9Export
from antlir.vm.vm import ConsoleRedirect, ShellMode, vm, VMExecOpts
from antlir.vm.vm_opts_t import vm_opts_t


logger = get_logger()


def blocking_print(*args, file: io.IOBase = sys.stdout, **kwargs):
    blocking = os.get_blocking(file.fileno())
    os.set_blocking(file.fileno(), True)
    print(*args, file=file, **kwargs)
    # reset to the old blocking mode
    os.set_blocking(file.fileno(), blocking)


class VMTestExecOpts(VMExecOpts):
    """
    Custom execution options for this VM entry point.
    """

    devel_layer: bool = False
    setenv: List[str] = []
    sync_file: List[Path] = []
    test_binary: Path
    test_binary_image: Path
    gtest_list_tests: bool
    list_tests: Optional[str]
    list_rust: bool

    @classmethod
    def setup_cli(cls, parser):
        super(VMTestExecOpts, cls).setup_cli(parser)

        parser.add_argument(
            "--devel-layer",
            action="store_true",
            default=False,
            help="Provide the kernel devel layer as a mount to the booted VM",
        )
        parser.add_argument(
            "--setenv",
            action="append",
            default=[],
            help="Specify an environment variable to pass to the test "
            "in the form NAME=VALUE",
        )

        parser.add_argument(
            "--sync-file",
            type=Path,
            action="append",
            default=[],
            help="Sync this file for tpx from the vm to the host.",
        )
        parser.add_argument(
            "--test-binary",
            type=Path,
            help="Path to the actual test binary that will be invoked.  This "
            "is used to discover tests before they are executed inside the VM",
            required=True,
        )
        parser.add_argument(
            "--test-binary-image",
            type=Path,
            help="Path to a btrfs loopback image that contains the test binary "
            "to run",
            required=True,
        )
        list_group = parser.add_mutually_exclusive_group()
        list_group.add_argument(
            "--gtest_list_tests",
            action="store_true",
        )  # For c++ gtest
        list_group.add_argument(
            "--list-tests",
        )  # Python pyunit with the new TestPilot adapter
        list_group.add_argument(
            "--list", action="store_true", dest="list_rust"
        )  # Rust


async def run(
    # common args from VMExecOpts
    bind_repo_ro: bool,
    console: ConsoleRedirect,
    debug: bool,
    extra: List[str],
    opts: vm_opts_t,
    shell: Optional[ShellMode],
    timeout_ms: int,
    # antlir.vm.vmtest specific args
    devel_layer: bool,
    gtest_list_tests: bool,
    list_tests: Optional[str],
    list_rust: bool,
    setenv: List[str],
    sync_file: List[str],
    test_binary: Path,
    test_binary_image: Path,
) -> None:

    # Start the test binary directly to list out test cases instead of
    # starting an entire VM.  This is faster, but it's also a potential
    # security hazard since the test code may expect that it always runs
    # sandboxed, and may run untrusted code as part of listing tests.
    # TODO(vmagro): the long-term goal should be to make vm boots as
    # fast as possible to avoid unintuitive tricks like this
    if gtest_list_tests or list_tests or list_rust:
        assert (
            int(gtest_list_tests) + int(bool(list_tests)) + int(list_rust)
        ) == 1, "Got mutually exclusive test listing arguments"
        args = []
        if gtest_list_tests:
            args = ["--gtest_list_tests"]
        elif list_tests:
            # NB: Unlike for the VM, we don't explicitly have to
            # pass the magic `TEST_PILOT` environment var to allow
            # triggering the new TestPilotAdapter. The environment
            # is inherited.
            args = ["--list-tests", list_tests]
        elif list_rust:
            args = ["--list"]
        proc = await asyncio.create_subprocess_exec(str(test_binary), *args)
        await proc.wait()
        sys.exit(proc.returncode)

    # If we've made it this far we are executing the actual test, not just
    # listing tests
    returncode = -1
    test_env = dict(s.split("=", maxsplit=1) for s in setenv)

    # Build shares to provide to the vm
    shares = [BtrfsDisk(test_binary_image, "/vmtest")]
    if devel_layer and opts.kernel.artifacts.devel is None:
        raise Exception(
            "--devel-layer requires kernel.artifacts.devel set in vm_opts"
        )
    if devel_layer:
        shares += [
            Plan9Export(
                path=opts.kernel.artifacts.devel.subvol.path(),
                mountpoint="/usr/src/kernels/{}".format(opts.kernel.uname),
                mount_tag="kernel-devel-src",
                generator=True,
            ),
            Plan9Export(
                path=opts.kernel.artifacts.devel.subvol.path(),
                mountpoint="/usr/lib/modules/{}/build".format(
                    opts.kernel.uname
                ),
                mount_tag="kernel-devel-build",
                generator=True,
            ),
        ]

    async with vm(
        bind_repo_ro=bind_repo_ro,
        console=console,
        opts=opts,
        shares=shares,
        shell=shell,
        timeout_ms=timeout_ms,
    ) as (instance, boot_elapsed_ms, timeout_ms):

        # If we are run with `--shell` mode, we don't get an instance since
        # the --shell mode takes over.  This is a bit of a wart that exists
        # because if a context manager doesn't yield *something* it will
        # throw an exception that this caller has to handle.
        if instance:
            # Sync the file which tpx needs from the vm to the host.
            file_arguments = list(sync_file)
            for arg in extra:
                # for any args that look like files make sure that the
                # directory exists so that the test binary can write to
                # files that it expects to exist (that would normally be
                # created by TestPilot)
                dirname = os.path.dirname(arg)
                # TestPilot will already create the directories on the
                # host, so as another sanity check only create the
                # directories in the VM that already exist on the host
                if dirname and os.path.exists(dirname):
                    await instance.run(
                        ["mkdir", "-p", dirname],
                        timeout_ms=timeout_ms,
                    )
                    file_arguments.append(arg)

            # The behavior of the FB-internal Python test main changes
            # completely depending on whether this environment var is set.
            # We must forward it so that the new TP adapter can work.
            test_pilot_env = os.environ.get("TEST_PILOT")
            if test_pilot_env:
                test_env["TEST_PILOT"] = test_pilot_env

            cmd = ["/vmtest/test"] + list(extra)
            logger.debug(f"executing {cmd} inside guest")
            returncode, stdout, stderr = await instance.run(
                cmd=cmd,
                timeout_ms=timeout_ms,
                env=test_env,
                # TODO(lsalis):  This is currently needed due to how some
                # cpp_unittest targets depend on artifacts in the code
                # repo.  Once we have proper support for `runtime_files`
                # this can be removed.  See here for more details:
                # https://fburl.com/xt322rks
                cwd=find_buck_cell_root(path_in_repo=Path(os.getcwd())),
            )

            if returncode != 0:
                logger.error(f"{cmd} failed with returncode {returncode}")
            else:
                logger.debug(f"{cmd} succeeded")

            # Some tests have incredibly large amounts of output, which
            # results in a BlockingIOError when stdout/err are in
            # non-blocking mode. Just force it to print the output in
            # blocking mode to avoid that - we don't really care how long
            # it ends up blocked as long as it eventually gets written.
            if stdout:
                blocking_print(stdout.decode("utf-8"), end="")

            if stderr:
                blocking_print(stderr.decode("utf-8"), file=sys.stderr, end="")

            for path in file_arguments:
                logger.debug(f"copying {path} back to the host")
                # copy any files that were written in the guest back to the
                # host so that TestPilot can read from where it expects
                # outputs to end up
                try:
                    retcode, contents, _ = await instance.run(
                        ["cat", str(path)],
                        check=True,
                        timeout_ms=timeout_ms,
                    )
                    with open(path, "wb") as out:
                        out.write(contents)
                except Exception as e:
                    logger.error(f"Failed to copy {path} to host: {str(e)}")

    sys.exit(returncode)


if __name__ == "__main__":
    asyncio.run(run(**dict(VMTestExecOpts.parse_cli(sys.argv[1:]))))
