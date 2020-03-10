#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
This is intended to execute a Buck test target from inside an `image.layer`
container.

At a high level, this is very similar identical to `run.py`, so read that
file's docblock for a an introduction.  Also try `--help` for the `run-test`
binary.  You will note that some options are missing compared to `run`, and
that a couple of test-runner specific options are added.

This test wrapper expects to run a specific command, `/layer-test-binary`,
to exist inside the image, and takes the liberty of rewriting some of its
arguments, as documented in `rewrite_test_cmd`.
'''
import argparse
import os
import shlex
import sys

from contextlib import contextmanager
from typing import List, Tuple

from .args import _parse_cli_args
from .cmd import PopenArgs
from .booted import run_booted_nspawn
from .non_booted import run_non_booted_nspawn


@contextmanager
def rewrite_test_cmd(cmd: List[str], next_fd: int) -> Tuple[List[str], int]:
    '''
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
    '''
    # This should only used only for image unit-tests, so check the binary path.
    assert cmd[0] == '/layer-test-binary', cmd

    # Our partial parser must not accept abbreviated long options like
    # `--ou`, since this parser does not know all the test main arguments.
    parser = argparse.ArgumentParser(allow_abbrev=False)

    # Future: these options may be specific to `python_unittest`.
    parser.add_argument('--output', '-o')
    parser.add_argument('--list-tests')
    test_opts, unparsed_args = parser.parse_known_args(cmd[1:])

    if test_opts.output is None and test_opts.list_tests is None:
        yield cmd, None
        return

    # we don't expect both --output and --list-tests
    assert test_opts.output is None or test_opts.list_tests is None, cmd

    if test_opts.output:
        output_file = test_opts.output
        output_opt = '--output'
    else:
        assert test_opts.list_tests
        output_file = test_opts.list_tests
        output_opt = '--list-tests'

    with open(output_file, 'wb') as f:
        # It's not great to assume that the container has a `/bin/bash`, but
        # eliminating this dependency is low-priority since current test
        # binaries will depend on it, too (PAR).
        yield ['/bin/bash', '-c', ' '.join([
            'exec',  # Try to save a wrapper
            shlex.quote(cmd[0]),
            # We cannot just pass `/proc/self/fd/{next_fd}` as the path,
            # even though that's technically a functional path.  The catch
            # is that the permissions to `open` this path will be those of
            # the original file -- owned by the `buck test` user.  But we
            # want the container user to be able to open it.  So this `cat`
            # here straddles a privilege boundary.
            output_opt, f'>(cat >&{next_fd})',
            *(shlex.quote(arg) for arg in unparsed_args),
        ])], f.fileno()


# Integration coverage is provided by `image.python_unittest` targets, which
# use `nspawn_in_subvol/run_test.py` in their implementation.  However, here
# is a basic smoke test, which, incidentally, demonstrates our test error
# handling is sane since `/layer-test-binary` is absent in that image,
# causing the container to exit with code 1.
#
#   buck run //fs_image/nspawn_in_subvol:run-test -- --layer "$(
#     buck build --show-output \
#       //fs_image/compiler/test_images:only-for-tests-read-only-host-clone |
#         cut -f 2- -d ' '
#   )" -- /layer-test-binary -ba r --baz=3 --output $(mktemp) --ou ; echo $?
#
if __name__ == '__main__':  # pragma: no cover
    argv = []

    # Propagate env vars used by FB test runner
    # /!\ /!\ /!\
    # When editing these lines, make sure you are not breaking test pilot
    # behaviour and you are not letting test targets pass even when they
    # should fail. Also check tests are properly discovered.
    for k, v in os.environ.items():
        if k.startswith('TEST_PILOT'):
            argv.extend(['--setenv', f'{k}={v}'])

    # When used as part of the `image.python_unittest` implementation, there
    # is no good way to pass arguments to this nspawn wrapper.  So, we
    # package the `image.layer` as a resource, and the remaining arguments
    # as Python source module.  These are optional only to allow the kind of
    # manual test shown above.
    packaged_layer = os.path.join(
        os.path.dirname(__file__), 'nspawn-in-test-subvol-layer',
    )
    if os.path.exists(packaged_layer):
        argv.extend(['--layer', packaged_layer])
        from fs_image.nspawn_in_subvol import __image_python_unittest_spec__
        argv.extend(__image_python_unittest_spec__.nspawn_in_subvol_args())

    args = _parse_cli_args(argv + sys.argv[1:], allow_debug_only_opts=False)

    with rewrite_test_cmd(
        args.opts.cmd, next_fd=3 + len(args.opts.forward_fd),
    ) as (new_cmd, fd_to_forward):
        ret = (
            run_booted_nspawn if args.boot else run_non_booted_nspawn
        )(
            args.opts._replace(
                cmd=new_cmd,
                forward_fd=args.opts.forward_fd + (
                    [] if fd_to_forward is None else [fd_to_forward]
                ),
            ),
            PopenArgs(
                check=False,  # We forward the return code below
                # By default, our internal `Popen` analogs redirect `stdout`
                # to `stderr` to protect stdout from subprocess spam.  Undo
                # that, since we want this CLI to be usable in pipelines.
                stdout=1,
                # Default `boot_console` -- for booted containers, let the
                # console go to stderr so that tests are easier to debug.
            ),
        )

    # Only trigger SystemExit after the context was cleaned up.
    sys.exit(ret.returncode)
