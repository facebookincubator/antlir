#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
This is intended to execute a Buck test target from inside an `image.layer`
container.

At a high level, this is very similar identical to `run.py`, so read that
file's docblock for a an introduction.  Also try `--help` for the `run-test`
binary.  You will note that some options are missing compared to `run`, and
that a couple of test-runner specific options are added.

This test wrapper expects to run a specific command, `/layer-test-binary`,
to exist inside the image, and takes the liberty of rewriting some of its
arguments, as documented in `rewrite_test_cmd`.
"""
import argparse
import os
import shlex
import sys
from contextlib import contextmanager
from typing import Dict, Iterable, List, Tuple

from antlir.fs_utils import Path

from .cmd import PopenArgs
from .run import _set_up_run_cli


def forward_env_vars(environ: Dict[str, str]) -> Iterable[str]:
    """
    Propagate into the test container the environment variables that are
    required for test infra & debugging.

    IMPORTANT: Only add things here that have a minimal likelihood of
    breaking test reproducibility.
    """
    for k, v in environ.items():
        if (
            k.startswith(
                # IMPORTANT: When editing this line, make sure you are not
                # breaking TPX / TestPilot behaviour and you are not letting
                # test targets pass even when they should fail.  Also check
                # that tests are properly discovered.
                "TEST_PILOT"
            )
            or k == "ANTLIR_DEBUG"
        ):
            yield f"--setenv={k}={v}"


@contextmanager
def do_not_rewrite_cmd(
    cmd: List[str], next_fd: int
) -> Tuple[List[str], List[int]]:
    # pyre-fixme[7]: Expected `Tuple[List[str], List[int]]` but got
    #  `Generator[Tuple[List[str], List[Variable[_T]]], None, None]`.
    yield cmd, []


@contextmanager
def rewrite_testpilot_python_cmd(
    cmd: List[str], next_fd: int
) -> Tuple[List[str], List[int]]:
    """
    The TestPilot CLI interface can have a `--output PATH` or `--list-tests`
    option, which requires us to exfiltrate data from inside the container
    to the host.

    There are a couple of complications with this:
      - The user running `buck test` will commonly be different from the
        user excuting the test inside the container, yet they need to access
        the same file.  It would not be ideal to mark the output path
        world-writable to make this work.  Nor would it be great to be doing
        a bunch of `chmod`s as `root` -- consider that it's actually not
        trivial to figure out what the UID of the requested container user
        will be.
      - Since the test runner controls the path, it's not ideal to have to
        create a matching directory hiearchy it in the container.

    To deal with these, To achieve this, the current function:
      - (partially) parses `cmd`,
      - opens PATH for writing,
      - forwards the resulting FD into the container, and injects an
        accessor for the received FD into the test's command-line.
    """
    # Our partial parser must not accept abbreviated long options like
    # `--ou`, since this parser does not know all the test main arguments.
    parser = argparse.ArgumentParser(allow_abbrev=False, add_help=False)

    # Future: these options may be specific to `python_unittest`.
    parser.add_argument("--output", "-o")
    parser.add_argument("--list-tests", nargs="?")
    test_opts, unparsed_args = parser.parse_known_args(cmd[1:])

    if test_opts.output is None and test_opts.list_tests is None:
        # pyre-fixme[7]: Expected `Tuple[List[str], List[int]]` but got
        #  `Generator[Tuple[List[str], List[Variable[_T]]], None, None]`.
        yield cmd, []
        # pyre-fixme[7]: Expected `Tuple[List[str], List[int]]` but got
        #  `Generator[typing.Any, typing.Any, None]`.
        return

    # we don't expect both --output and --list-tests
    assert test_opts.output is None or test_opts.list_tests is None, cmd

    if test_opts.output:
        output_file = test_opts.output
        output_opt = "--output"
    else:
        assert test_opts.list_tests
        output_file = test_opts.list_tests
        output_opt = "--list-tests"

    with open(output_file, "wb") as f:
        # It's not great to assume that the container has a `/bin/bash`, but
        # eliminating this dependency is low-priority since current test
        # binaries will depend on it, too (PAR).
        # pyre-fixme[7]: Expected `Tuple[List[str], List[int]]` but got
        #  `Generator[Tuple[List[str], List[int]], None, None]`.
        yield [
            "/bin/bash",
            "-c",
            " ".join(
                [
                    "exec",  # Try to save a wrapper
                    shlex.quote(cmd[0]),
                    # We cannot just pass `/proc/self/fd/{next_fd}` as the path,
                    # even though that's technically a functional path.  The
                    # catch is that the permissions to `open` this path will be
                    # those of the original file -- owned by the `buck test`
                    # user.  But we want the container user to be able to open
                    # it.  So this `cat` here straddles a privilege boundary.
                    output_opt,
                    f">(cat >&{next_fd})",
                    *(shlex.quote(arg) for arg in unparsed_args),
                ]
            ),
        ], [f.fileno()]


@contextmanager
def rewrite_tpx_gtest_cmd(
    cmd: List[str], next_fd: int
) -> Tuple[List[str], List[int]]:
    """
    The new test runner TPX expects gtest to write XML to a file specified
    by an environment variable. We'll give the test container access to the
    host's output file by forwarding an FD into the container.
    """
    gtest_output = os.environ.get("GTEST_OUTPUT")
    if gtest_output is None:
        # pyre-fixme[7]: Expected `Tuple[List[str], List[int]]` but got
        #  `Generator[Tuple[List[str], List[Variable[_T]]], None, None]`.
        yield cmd, []
        # pyre-fixme[7]: Expected `Tuple[List[str], List[int]]` but got
        #  `Generator[typing.Any, typing.Any, None]`.
        return

    # TPX only uses XML output, so fail on anything else.
    prefix = "xml:"
    assert gtest_output.startswith(prefix)
    gtest_output = gtest_output[len(prefix) :]

    with open(gtest_output, "wb") as f:
        # It's not great to assume that the container has a `/bin/bash`, but
        # eliminating this dependency is low-priority since current test
        # binaries will depend on it, too (PAR).
        # pyre-fixme[7]: Expected `Tuple[List[str], List[int]]` but got
        #  `Generator[Tuple[List[str], List[int]], None, None]`.
        yield [
            "/bin/bash",
            "-c",
            " ".join(
                [
                    # We cannot just pass `/proc/self/fd/{next_fd}` as the path,
                    # even though that's technically a functional path.  The
                    # catch is that the permissions to `open` this path will be
                    # those of the original file -- owned by the `buck test`
                    # user.  But we want the container user to be able to open
                    # it.  So this `cat` here straddles a privilege boundary.
                    f"GTEST_OUTPUT={shlex.quote(prefix)}>(cat >&{next_fd})",
                    "exec",  # Try to save a wrapper
                    *(shlex.quote(c) for c in cmd),
                ]
            ),
        ], [f.fileno()]


_TEST_TYPE_TO_REWRITE_CMD = {
    "pyunit": rewrite_testpilot_python_cmd,
    "gtest": rewrite_tpx_gtest_cmd,
    "rust": do_not_rewrite_cmd,
}


@contextmanager
def add_container_not_part_of_build_step(argv):
    with Path.resource(__package__, "nis_domainname", exe=True) as p:
        yield [f"--container-not-part-of-build-step={p}", *argv]


# Integration coverage is provided by `image.python_unittest` targets, which
# use `nspawn_in_subvol/run_test.py` in their implementation.  However, here
# is a basic smoke test, which, incidentally, demonstrates our test error
# handling is sane since `/layer-test-binary` is absent in that image,
# causing the container to exit with code 1.
#
#   buck run //antlir/nspawn_in_subvol:run-test -- --layer "$(
#     buck build --show-output \
#       //antlir/compiler/test_images:test-layer |
#         cut -f 2- -d ' '
#   )" -- /layer-test-binary -ba r --baz=3 --output $(mktemp) --ou ; echo $?
#
if __name__ == "__main__":  # pragma: no cover
    argv = []

    # pyre-fixme[6]: Expected `Dict[str, str]` for 1st param but got
    # `_Environ[str]`.
    argv.extend(forward_env_vars(os.environ))

    # When used as part of the `image.python_unittest` implementation, there
    # is no good way to pass arguments to this nspawn wrapper.  So, we
    # package the `image.layer` as a resource, and the remaining arguments
    # as Python source module.  These are optional only to allow the kind of
    # manual test shown above.
    packaged_layer = os.path.join(
        os.path.dirname(__file__), "nspawn-in-test-subvol-layer"
    )
    if os.path.exists(packaged_layer):
        argv.extend(["--layer", packaged_layer])
        # pyre-fixme[21]: Could not find name `__image_python_unittest_spec__`
        #  in `antlir.nspawn_in_subvol`.
        from antlir.nspawn_in_subvol import __image_python_unittest_spec__

        # pyre-fixme[16]: Module `nspawn_in_subvol` has no attribute
        #  `__image_python_unittest_spec__`.
        argv.extend(__image_python_unittest_spec__.nspawn_in_subvol_args())
        rewrite_cmd = _TEST_TYPE_TO_REWRITE_CMD[
            # pyre-fixme[16]: Module `nspawn_in_subvol` has no attribute
            #  `__image_python_unittest_spec__`.
            __image_python_unittest_spec__.TEST_TYPE
        ]
    else:
        rewrite_cmd = do_not_rewrite_cmd  # Used only for the manual test

    # pyre-fixme[16]: `_CliSetup` has no attribute `__enter__`.
    # pyre-fixme[16]: `Tuple` has no attribute `__enter__`.
    with add_container_not_part_of_build_step(argv) as argv, _set_up_run_cli(
        argv
        + sys.argv[1:]
        # pyre-fixme[16]: `Tuple` has no attribute `__enter__`.
    ) as cli_setup, rewrite_cmd(
        cli_setup.opts.cmd, next_fd=3 + len(cli_setup.opts.forward_fd)
    ) as (
        new_cmd,
        fds_to_forward,
    ):

        # This should only used only for `image.*_unittest` targets.
        assert cli_setup.opts.cmd[0] == "/layer-test-binary.par"
        # Always use the default `console` -- let it go to stderr so that
        # tests are easier to debug.
        assert cli_setup.console is None
        ret, _boot_ret = cli_setup._replace(
            opts=cli_setup.opts._replace(
                cmd=new_cmd,
                # pyre-fixme[60]: Concatenation not yet support for multiple
                #  variadic tuples: `*cli_setup.opts.forward_fd,
                #  *fds_to_forward`. pyre-fixme[60]: Expected to unpack an
                #  iterable, but got `unknown`.
                forward_fd=(*cli_setup.opts.forward_fd, *fds_to_forward),
            )
        )._run_nspawn(
            PopenArgs(
                check=False,  # We forward the return code below
                # By default, our internal `Popen` analogs redirect `stdout`
                # to `stderr` to protect stdout from subprocess spam.  Undo
                # that, since we want this CLI to be usable in pipelines.
                stdout=1,
            )
        )

    # Only trigger SystemExit after the context was cleaned up.
    sys.exit(ret.returncode)
