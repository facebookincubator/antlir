#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
When developing images, it is very handy to be able to run code inside an
image.  This target lets you do just that, for example, here is a shell:

    buck run //antlir/nspawn_in_subvol:run -- --layer "$(
        buck build --show-output \\
            //antlir/compiler/test_images:test-layer |
                cut -f 2- -d ' ')"

The above is a handful to remember, so each layer gets a corresponding
`=container` target.  To be used like so:

    buck run //PATH/TO:SOME_LAYER=container  # Runs `bash` by default
    buck run //PATH/TO:SOME_LAYER=container -- -- printenv

Note that there are two sets of `--`.  The first separates `buck run`
arguments from those of the container runtime.  The second separates the
container args from the in-container command.

Note: If no command is passed to systemd-nspawn to execute, then the
default behavior is to invoke a shell. `/bin/bash` is tried first and if
that is not found then `/bin/sh` is used.  We use this default behavior
to provide a shell as the default when `buck run` is used as in the
example above.

IMPORTANT: This is NOT READY to use as a sandbox for build steps.  The
reason is that `systemd-nspawn` does a bunch of random things to the
filesystem, which we would need to explicitly control (see "Filesystem
mutations" below).


## Known issues

  - The `hostname` of the container is not currently set to a useful value,
    which can affect some network operations.

  - T40937041: If `stdout` is a PTY, then `stderr` redirection does not work
    -- the container's `stderr` will also point at the PTY.  This is an
    nspawn bug, and working around it form this wrapper would be hard.  This
    issue was fixed in systemd 242.

  - T40936918: At present, `nspawn` prints a spurious newline to stdout,
    even if `stdout` is redirected.  This is due to an errant `putc('\\n',
    stdout);` in `nspawn.c`.  This will most likely be fixed in future
    releases of systemd.  I could work around this in the wrapper by passing
    `--quiet` when `not sys.stdout.isatty()`.  However, that loses valuable
    debugging output, so I'm not doing it yet.  This issue was fixed in
    systemd 242.


## What does nspawn do, roughly?

This section is as of systemd 238/239, and will never be 100% perfect.  For
production-readiness, we would want to write automatic tests of nspawn's
behavior (especially, against minimal containers) to ensure future `systemd`
releases don't surprise us by creating yet-more filesystem objects.


### Isolates all specified kernel namespaces

  - pid
  - mount
  - network with --private-network
  - uts & ipc
  - cgroup (if supported by the base system)
  - user (if requested, we don't request it below due to kernel support)


### Filesystem mutations and requirements

`nspawn` will refuse to use a directory unless these two exist:
  - `/usr/`
  - an `os-release` file

`nspawn` will always ensure these exist before starting its container:
  - /dev
  - /etc
  - /lib will symlink to /usr/lib if the latter exists, but the former does not
  - /proc
  - /root -- permissions nonstandard, should be 0700 not 0755.
  - /run
  - /sys
  - /tmp
  - /var/log/journal/

`nspawn` wants to modify `/etc/resolv.conf` if `--private-network` is off.

The permissions of the created directories seem to be 0755 by default, and
all are owned by root (except for $HOME which may depend if we vary the
user, which we should probably never do).


## Future

  - Which capabilities can we drop?  Note that CAP_NET_ADMIN might be needed
    to set up `--private-network` interfaces.

  - Can we get any mileage out of --system-call-filter?

"""
import subprocess
from contextlib import contextmanager, nullcontext
from io import BytesIO
from typing import Iterable, NamedTuple, Tuple, Union

from antlir.common import init_logging
from antlir.fs_utils import Path

from .args import _NspawnOpts, _parse_cli_args, PopenArgs
from .common import UserFacingError
from .nspawn import run_nspawn
from .plugins import NspawnPlugin
from .plugins.repo_plugins import repo_nspawn_plugins


class _CliSetup(NamedTuple):
    console: BytesIO
    opts: _NspawnOpts
    plugins: Iterable[NspawnPlugin]

    def _run_nspawn(
        self,
        popen_args: PopenArgs
        # pyre-fixme[24]: Generic type `subprocess.CompletedProcess` expects 1 type
        #  parameter.
    ) -> Tuple[subprocess.CompletedProcess, subprocess.CompletedProcess]:
        # Enforce a single source of truth for `PopenArgs.console`.
        assert (
            popen_args.console is None
        ), "To set `console`, use `_CliSetup._replace(console=)`."
        return run_nspawn(
            self.opts,
            popen_args._replace(console=self.console),
            plugins=self.plugins,
        )


@contextmanager
def _set_up_run_cli(argv: Iterable[Union[str, Path]]) -> _CliSetup:
    args = _parse_cli_args(argv, allow_debug_only_opts=True)
    # pyre-fixme[16]: `_NspawnOpts` has no attribute `opts`.
    init_logging(debug=args.opts.debug_only_opts.debug)
    with (
        # By default, the console output is supressed or sent to
        # stderr, otherwise we send it to a file.
        # pyre-fixme[16]: `_NspawnOpts` has no attribute `append_console`.
        open(args.append_console, "a")
        if isinstance(args.append_console, Path)
        else nullcontext(enter_result=args.append_console)
    ) as console:
        # pyre-fixme[7]: Expected `_CliSetup` but got `Generator[_CliSetup,
        #  None, None]`.
        yield _CliSetup(
            console=console,
            opts=args.opts,
            plugins=repo_nspawn_plugins(
                opts=args.opts,
                # pyre-fixme[16]: `_NspawnOpts` has no attribute `plugin_args`.
                plugin_args=args.plugin_args,
            ),
        )


# The manual test is in the first paragraph of the top docblock.
if __name__ == "__main__":  # pragma: no cover
    import sys

    try:
        # pyre-fixme[16]: `_CliSetup` has no attribute `__enter__`.
        with _set_up_run_cli(sys.argv[1:]) as cli_setup:
            # pyre-fixme[5]: Global expression must be annotated.
            ret, _boot_ret = cli_setup._run_nspawn(
                PopenArgs(
                    check=False,  # We forward the return code below
                    # By default, our internal `Popen` analogs redirect `stdout`
                    # to `stderr` to protect stdout from subprocess spam.  Undo
                    # that, since we want this CLI to be usable in pipelines.
                    stdout=1,
                )
            )
    except UserFacingError as e:
        sys.exit(e)
    sys.exit(ret.returncode)
