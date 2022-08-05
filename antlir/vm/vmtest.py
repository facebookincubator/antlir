#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import argparse
import asyncio
import contextlib
import logging
import os.path
import sys
import uuid
from typing import Any, AsyncGenerator, Dict, List, Optional, Tuple, Union

from antlir.artifacts_dir import find_buck_cell_root
from antlir.cli import normalize_buck_path
from antlir.common import get_logger, not_none
from antlir.config import repo_config
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import Path
from antlir.vm.bzl.vm import vm_opts_t
from antlir.vm.share import Plan9Export
from antlir.vm.vm import ConsoleRedirect, ShellMode, vm, VMExecOpts


log = get_logger()


@contextlib.asynccontextmanager
async def wrap_and_forward(
    output: Path, cmd: List[Union[Path, str]], test_type: str, wrapper: Path
):
    with open(output, "wb") as out:
        stop = asyncio.Event()

        async def _handle(reader, writer):
            while True:
                data = await reader.read(4096)
                if not data and reader.at_eof():
                    break
                out.write(data)
                out.flush()

            writer.close()
            await writer.wait_closed()
            stop.set()

        socket = Path("/tmp") / f"{uuid.uuid4().hex}.sock"
        async with await asyncio.start_unix_server(_handle, path=str(socket)):
            try:
                yield [
                    wrapper,
                    "--socket",
                    socket,
                    "--test-type",
                    test_type,
                    "--",
                    *cmd,
                ], socket

                await stop.wait()
            finally:
                os.unlink(socket)


@contextlib.asynccontextmanager
async def do_not_wrap_cmd(
    cmd: List[Union[Path, str]], env: Dict[Any, Any], wrapper: Path
) -> AsyncGenerator[
    Tuple[List[Union[Path, str]], Dict[Any, Any], Optional[Path]], None
]:
    yield cmd, env, None


@contextlib.asynccontextmanager
async def wrap_testpilot_python_cmd(
    cmd: List[Union[Path, str]],
    env: Dict[Any, Any],
    wrapper: Path,
) -> AsyncGenerator[
    Tuple[List[Union[Path, str]], Dict[Any, Any], Optional[Path]], None
]:
    parser = argparse.ArgumentParser(allow_abbrev=False, add_help=False)
    parser.add_argument("--output", "-o")

    # pyre-fixme[6]: Expected `Optional[typing.Sequence[str]]` for 1st param
    #   but got `List[Union[Path, str]]`.
    test_opts, unparsed_args = parser.parse_known_args(cmd[1:])

    if not test_opts.output:
        yield cmd, env, None
    else:
        async with wrap_and_forward(
            output=test_opts.output,
            cmd=[cmd[0]] + unparsed_args,
            test_type="pyunit",
            wrapper=wrapper,
        ) as (cmd, socket):
            yield cmd, env, socket


@contextlib.asynccontextmanager
async def wrap_tpx_gtest_cmd(
    cmd: List[Union[Path, str]],
    env: Dict[Any, Any],
    wrapper: Path,
) -> AsyncGenerator[
    Tuple[List[Union[Path, str]], Dict[Any, Any], Optional[Path]], None
]:
    log.debug("Rewriting gtest cmd: {cmd}")

    gtest_output = env.get("GTEST_OUTPUT") or os.environ.get("GTEST_OUTPUT")
    if not gtest_output:
        yield cmd, env, None
    else:
        prefix = "xml:"
        assert gtest_output.startswith(prefix)
        gtest_output = gtest_output[len(prefix) :]
        async with wrap_and_forward(
            output=gtest_output,
            cmd=cmd,
            test_type="gtest",
            wrapper=wrapper,
        ) as (cmd, socket):
            yield cmd, env, socket


_TEST_TYPE_TO_WRAP_CMD = {
    "pyunit": wrap_testpilot_python_cmd,
    "gtest": wrap_tpx_gtest_cmd,
    "rust": do_not_wrap_cmd,
}


# pyre-fixme[13]: Attributes `test_binary`, `test_binary_image`, and
#   `test_type` are never initialized.
class VMTestExecOpts(VMExecOpts):
    """
    Custom execution options for this VM entry point.
    """

    devel_layer: bool = False
    setenv: List[str] = []
    gtest_list_tests: bool = False
    list_tests: Optional[Path] = None
    list_rust: bool = False
    test_binary: Path
    test_binary_wrapper: Path
    test_type: str

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
            "--test-binary",
            type=normalize_buck_path,
            help="Path to the actual test binary that will be invoked.  This "
            "is used to discover tests before they are executed inside the VM",
            required=True,
        )
        parser.add_argument(
            "--test-binary-wrapper",
            type=normalize_buck_path,
            help="Path to the test binary wrapper",
            required=True,
        )
        parser.add_argument(
            "--test-type",
            help="The type of test being executed, this is populated "
            "by the .bzl that wraps the test.",
            required=True,
            choices=_TEST_TYPE_TO_WRAP_CMD.keys(),
        )

        list_group = parser.add_mutually_exclusive_group()
        # For gtest
        list_group.add_argument(
            "--gtest_list_tests",
            action="store_true",
        )
        # For python tests
        list_group.add_argument(
            "--list-tests",
            type=Path.from_argparse,
        )
        # For rust tests
        list_group.add_argument("--list", action="store_true", dest="list_rust")


async def run(
    # common args from VMExecOpts
    bind_repo_ro: bool,
    console: ConsoleRedirect,
    debug: bool,
    # Extra, unprocessed args passed to the CLI
    extra: List[str],
    opts: vm_opts_t,
    shell: Optional[ShellMode],
    timeout_ms: int,
    # antlir.vm.vmtest specific args
    devel_layer: bool,
    gtest_list_tests: bool,
    list_tests: Optional[Path],
    list_rust: bool,
    setenv: List[str],
    test_binary: Path,
    test_binary_wrapper: Path,
    test_type: str,
) -> Optional[int]:

    env = dict(s.split("=", maxsplit=1) for s in setenv)

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
        args: List[Union[Path, str]] = [test_binary]

        if gtest_list_tests:
            assert test_type == "gtest", (
                f"Incompatible test_type: {test_type} and list arg: "
                "--gtest_list_tests"
            )
            args += ["--gtest_list_tests"]
        # Python tests send output to the file provided by `--list-tests`
        elif list_tests:
            assert (
                test_type == "pyunit"
            ), f"Incompatible test_type: {test_type} and list arg: --list-tests"
            args += ["--list-tests", list_tests]
        elif list_rust:
            assert (
                test_type == "rust"
            ), f"Incompatible test_type: {test_type} and list arg: --list"
            args += ["--list"]

        log.debug(f"Listing tests: {args} to {list_tests}")
        output = Path("/dev/fd/1")
        with open(output, "wb") as f:
            proc = await asyncio.create_subprocess_exec(
                *args,
                stderr=f,
            )
            await proc.wait()
        return proc.returncode

    # If we've made it this far we are executing the actual test, not just
    # listing tests
    returncode = 0

    shares = []

    if devel_layer:
        devel_path = (
            not_none(
                find_built_subvol(opts.kernel.derived_targets.image.path)
            ).path()
            / "devel"
        )
        shares += [
            Plan9Export(
                path=devel_path,
                mountpoint=Path("/usr/src/kernels") / opts.kernel.uname,
                mount_tag="kernel-devel-src",
                generator=True,
            ),
            Plan9Export(
                path=devel_path,
                mountpoint=Path("/usr/lib/modules")
                / opts.kernel.uname
                / "build",
                mount_tag="kernel-devel-build",
                generator=True,
            ),
            Plan9Export(
                path=devel_path,
                mountpoint=Path("/usr/lib/modules")
                / opts.kernel.uname
                / "source",
                mount_tag="kernel-devel-modules-source",
                generator=True,
            ),
        ]

    buck_out_base_dir = repo_config().repo_root
    if os.environ["ANTLIR_BUCK"] != "buck2":
        buck_out_base_dir /= repo_config().antlir_cell_name

    async with vm(
        bind_repo_ro=bind_repo_ro,
        console=console,
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
        shares=shares,
        shell=shell,
        timeout_ms=timeout_ms,
    ) as (instance, boot_elapsed_ms, timeout_ms):

        # If we are run with `--shell` mode, we don't get an instance since
        # the --shell mode takes over.  This is a bit of a wart that exists
        # because if a context manager doesn't yield *something* it will
        # throw an exception that this caller has to handle.
        if instance:
            cmd: List[Union[Path, str]] = [test_binary]
            cmd.extend(list(extra))

            # find the correct rewrite command for the test type
            maybe_wrap_cmd = _TEST_TYPE_TO_WRAP_CMD[test_type]

            # Each test type (cpp, python, rust) has a different argument
            # format for defining where test output should go.  Additionally
            # since these tests are being executed *inside* the VM we need
            # to exfiltrate the test output somehow.  To avoid making multiple
            # connections to a test VM, we do the exfiltration using local
            # unix domain socket forwarding over the SSH connection.
            # Now, the average test binary cannot write their output directly
            # to a domain socket.  To handle that, the test binary (installed
            # at /vmtest/test) inside the VM has a special wrapper which
            # opens a new file descriptor, provides it to the test binary
            # when executed, and forwards the writes into the FD over the
            # domain socket.  On this end, we are provided with the domain
            # socket path to forward over the ssh connection.
            async with maybe_wrap_cmd(
                cmd=cmd, env=env, wrapper=test_binary_wrapper
            ) as (
                cmd,
                env,
                socket,
            ):
                log.debug(f"Executing {cmd} inside guest.")
                res = await instance.run(
                    cmd=cmd,
                    timeout_ms=timeout_ms,
                    env={
                        **env,
                        # Sets the magic var for the vmtest command, but not
                        # for the systemd units -- for that we append to the
                        # kernel command-line above.
                        "ANTLIR_CONTAINER_IS_NOT_PART_OF_A_BUILD_STEP": "1",
                    },
                    # Note: This is currently needed due to how some
                    # cpp_unittest targets depend on artifacts in the code
                    # repo.  Once we have proper support for `runtime_files`
                    # this can be removed.  See here for more details:
                    # https://fburl.com/xt322rks
                    cwd=buck_out_base_dir,
                    # Always dump stderr/stdout back to the calling terminal
                    stderr=None,
                    stdout=None,
                    # Maybe forward a socket
                    forward={socket: socket} if socket else None,
                )
                log.info(f"{cmd} completed with: {res.returncode}")
                returncode = res.returncode

    # Exit with the return code of the actual test run, not the VM exit
    return returncode


if __name__ == "__main__":
    # we don't want to terminate a test on simple logging errors
    logging.raiseExceptions = False
    asyncio.run(run(**dict(VMTestExecOpts.parse_cli(sys.argv[1:]))))
