#!/usr/bin/env python3
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

  - Should we drop CAP_NET_ADMIN, or any other capabilities?  Note that
    NET_ADMIN might be needed to set up `--private-network` interfaces.

  - Can we get any mileage out of --system-call-filter?

'''
import os
import subprocess
import sys

from fs_image.common import init_logging

from .args import _NspawnOpts, _parse_cli_args
from .cmd import _nspawn_setup
from .booted import _run_booted_nspawn
from .non_booted import _run_non_booted_nspawn


def nspawn_sanitize_env():
    env = os.environ.copy()
    # `systemd-nspawn` responds to a bunch of semi-private and intentionally
    # (mostly) undocumented environment variables.  Many of these can
    # compromise namespacing / isolation, which we emphatically do not want,
    # so let's prevent the ambient environment from changing them!
    #
    # Of course, this leaves alone a lot of the canonical variables
    # LINES/COLUMNS, or locale controls.  Those should be OK.
    for var in list(env.keys()):
        # No test coverage for this because (a) systemd does not pass such
        # environment vars to the container, so the only way to observe them
        # being set (or not) is via indirect side effects, (b) all the side
        # effects are annoying to test.
        if var.startswith('SYSTEMD_NSPAWN_'):  # pragma: no cover
            env.pop(var)
    return env


def nspawn_in_subvol(
    opts: _NspawnOpts, *,
    # Fixme: will be removed in a later diff, in favor of 2 separate functions.
    boot: bool,
    # These keyword-only arguments generally follow those of `subprocess.run`.
    #   - `check` defaults to True instead of False.
    #   - Unlike `run_as_root`, `stdout` is NOT default-redirected to `stderr`.
    stdout=None, stderr=None, check=True, quiet=False,
) -> subprocess.CompletedProcess:
    with _nspawn_setup(opts) as nspawn_setup:
        # We use a popen wrapper here to call popen_as_root and do the
        # necessary steps to run the command as root.
        #
        # Furthermore, we default stdout and stderr to the ones passed to this
        # function (and further default stdout to fd 1 if None is passed here.)
        #
        # Since we want to preserve the subprocess.Popen API (which take named
        # stdout and stderr arguments) and these arguments would get shadowed
        # here, let's pass the variables from the external scope in private
        # arguments _default_stdout and _default_stderr here. (This also gives
        # us a chance to default stdout to 1 if None is passed outside of the
        # inner function.)
        def popen(cmd, *, stdout=None, stderr=None,
                _default_stdout=1 if stdout is None else stdout,
                _default_stderr=stderr,
        ):
            return nspawn_setup.subvol.popen_as_root(
                cmd,
                # This is a safeguard in case `sudo` lets through these
                # unwanted environment variables.
                env=nspawn_sanitize_env(),
                # popen_as_root will redirect stdout to stderr if it is None,
                # don't do that because it will break things that don't
                # expect that.
                stdout=stdout if stdout else _default_stdout,
                stderr=stderr if stderr else _default_stderr,
                check=check,
            )

        if boot:
            return _run_booted_nspawn(nspawn_setup, popen)
        return _run_non_booted_nspawn(nspawn_setup, popen)


# The manual test is in the first paragraph of the top docblock.
if __name__ == '__main__':  # pragma: no cover
    args = _parse_cli_args(sys.argv[1:], allow_debug_only_opts=True)
    init_logging(debug=args.opts.debug_only_opts.debug)
    sys.exit(nspawn_in_subvol(
        args.opts, boot=args.boot, check=False,
    ).returncode)
