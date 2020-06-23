#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''
When developing images, it is very handy to be able to run code inside an
image.  This target lets you do just that, for example, here is a shell:

    buck run //fs_image/nspawn_in_subvol:run -- --layer "$(
        buck build --show-output \\
            //fs_image/compiler/tests:only-for-tests-read-only-host-clone |
                cut -f 2- -d ' ')"

The above is a handful to remember, so each layer gets a corresponding
`-container` target.  To be used like so:

    buck run //PATH/TO:SOME_LAYER-container  # Runs `bash` by default
    buck run //PATH/TO:SOME_LAYER-container -- -- printenv

Note that there are two sets of `--`.  The first separates `buck run`
arguments from those of the container runtime.  The second separates the
container args from the in-container command.

Note: If no command is passed to systemd-nspawn to execute, then the
defualt behavior is to invoke a shell. `/bin/bash` is tried first and if
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

'''
import functools
import subprocess

from contextlib import contextmanager
from typing import Iterable, NamedTuple, Optional, Tuple
from io import BytesIO

from fs_image.common import init_logging, nullcontext

from .args import _NspawnOpts, _parse_cli_args, PopenArgs
from .booted import run_booted_nspawn
from .inject_repo_servers import nspawn_plugin_to_inject_repo_servers
from .inject_yum_dnf_versionlock import (
    nspawn_plugin_to_inject_yum_dnf_versionlock,
)
from .non_booted import run_non_booted_nspawn
from .plugins import NspawnPlugin


class _CliSetup(NamedTuple):
    boot: bool
    boot_console: BytesIO
    opts: _NspawnOpts
    nspawn_plugins: Iterable[NspawnPlugin]

    def _run_nspawn(self, popen_args: PopenArgs) -> Tuple[
        subprocess.CompletedProcess, Optional[subprocess.CompletedProcess],
    ]:
        # Enforce a single source of truth for `PopenArgs.boot_console`.
        assert popen_args.boot_console is None, \
            'To set `boot_console`, use `_CliSetup._replace(boot_console=)`.'
        res = (
            run_booted_nspawn if self.boot else run_non_booted_nspawn
        )(
            self.opts,
            popen_args._replace(boot_console=self.boot_console),
            plugins=self.nspawn_plugins,
        )
        return res if self.boot else (res, None)


@contextmanager
def _set_up_run_cli(argv: Iterable[str]) -> _CliSetup:
    args = _parse_cli_args(argv, allow_debug_only_opts=True)
    init_logging(debug=args.opts.debug_only_opts.debug)
    with (
        open(args.append_boot_console, 'ab')
            # By default, we send `systemd` console to `stderr`.
            if args.boot and args.append_boot_console else nullcontext()
    ) as boot_console:
        yield _CliSetup(
            boot=args.boot,
            boot_console=boot_console,
            opts=args.opts,
            nspawn_plugins=[
                nspawn_plugin_to_inject_yum_dnf_versionlock(
                    args.snapshot_to_versionlock,
                ),
                nspawn_plugin_to_inject_repo_servers(args.serve_rpm_snapshots),
            ] if args.serve_rpm_snapshots else [],
        )


# The manual test is in the first paragraph of the top docblock.
if __name__ == '__main__':  # pragma: no cover
    import sys
    with _set_up_run_cli(sys.argv[1:]) as cli_setup:
        ret, _boot_ret = cli_setup._run_nspawn(
            PopenArgs(
                check=False,  # We forward the return code below
                # By default, our internal `Popen` analogs redirect `stdout`
                # to `stderr` to protect stdout from subprocess spam.  Undo
                # that, since we want this CLI to be usable in pipelines.
                stdout=1,
            ),
        )
    sys.exit(ret.returncode)
