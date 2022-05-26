#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
`sudo -C` is forbidden by default, making it harder to pass file descriptors
to programs executed under `sudo`.

When used as a CLI with the `--sudo` argument (this is the normal usage),
you end up with the following chain of subprocess spawning:

(a) `send-fds-and-run` (this program) inherits `--fd`s from its caller, and
    `fork`+`exec`s...
(b) `sudo`, which closes all FDs except 0, 1, 2 and `fork`+`exec`s...
(c) (as the new user) `recv-fds-and-run` (this program's helper), which
    waits to receive the `--fd`s from `send-fds-and-run` via a Unix socket,
    remaps the `--fd`s to be sequential starting from FD 3, and `exec`s...
(d) (as the new user) the program being wrapped, which will now inherit the
    remapped `--fd`s from the helper.

Note that the process in (a) closes the FDs immediately after sending, (c)
self-destructs via exec, and (b) only holds references to FDs 0, 1, and 2
since `subprocess.Popen` defaults `close_fds` to True.  So, shortly after
(d) the program being wrapped starts, it will be the only holder of the FDs
out of (a-d).  NB: There is a (usually short) race between when (d) starts
and (a) finishes closing its FDs.

By default, the helper also sets `LISTEN_FDS` and `LISTEN_PID.  In the event
that the program being wrapped eventually `exec`s `systemd-nspawn`, these
environment variables will make `--fd`s available inside the container (they
will still be sequential starting from FD 3).  If the program being wrapped
does not exec `systemd-nspawn`, the environment variables will have no
effect on other systemd-related utilities, since they check `LISTEN_PID`.

Usage as a library involves calling `popen_and_inject_fds_after_sudo` to
wrap your post-`sudo` command.  See that function's docblock.
"""
import argparse
import array
import logging
import os
import socket
import subprocess
import sys
from contextlib import contextmanager

from .common import (
    FD_UNIX_SOCK_TIMEOUT,
    get_logger,
    init_logging,
    listen_temporary_unix_socket,
)
from .fs_utils import Path


log = get_logger()


# NB: This was copy-pasta'd from yum_dnf_from_snapshot.py
def send_fds(sock, fds) -> None:
    msg = b"unused"
    num_sent = sock.sendmsg(
        [msg],
        [
            (
                socket.SOL_SOCKET,
                socket.SCM_RIGHTS,
                array.array("i", fds).tobytes(),
                # Future: is `flags=socket.MSG_NOSIGNAL` a good idea?
            )
        ],
    )
    assert len(msg) == num_sent, (msg, num_sent)


@contextmanager
def popen_and_inject_fds_after_sudo(cmd, fds, popen, *, set_listen_fds: bool):
    """
    This is a context manager intended to let you imitate the as-CLI
    behavior documented in the module docblock.  See that docblock to
    understand the process-spawning chain.

      - `cmd` is the post-sudo command, to which you want to pass the FDs.
        This may invoke `systemd-nspawn`, in which case you will want to set
        `set_listen_fds` to `True` if you need the FDs to be delivered
        inside the container.
      - `fds` are arbitrary FDs that you must not close before this context
        manager produces a context variable.
      - `popen` is a callback returning a context manager that works just
        like `subprocess.Popen`.  The only difference is that normally this
        callback will prepend its arguments with a `['sudo', '--some',
        '--args', '--']` invocation.
    """
    with listen_temporary_unix_socket() as (lsock_path, lsock), Path.resource(
        __package__, "recv-fds-and-run", exe=True
    ) as recv_binary, popen(
        [
            # The wrapper is Python.  In @mode/dev this can end up writing
            # bytecode as `root` into `buck-out`, which would break Buck's
            # garbage-collection.  The magic environment variable fixes that.
            # This doesn't affect @mode/opt since that is precompiled anyway.
            "env",
            "PYTHONDONTWRITEBYTECODE=1",
            recv_binary,
            # Although the permissions of lsock_path restrict it to the repo
            # user, the wrapper runs as `root`, so it can connect.
            "--unix-sock",
            lsock_path,
            "--num-fds",
            str(len(fds)),
            *([] if set_listen_fds else ["--no-set-listen-fds"]),
            # The receive end should debug-log iff the send side does.
            *(["--debug"] if log.isEnabledFor(logging.DEBUG) else []),
            "--",
            *cmd,
        ]
    ) as proc:
        log.debug(f"Sending FDS {fds} to {cmd} via wrapper")
        lsock.settimeout(FD_UNIX_SOCK_TIMEOUT)
        with lsock.accept()[0] as csock:
            csock.settimeout(FD_UNIX_SOCK_TIMEOUT)
            send_fds(csock, fds)
        yield proc


def parse_opts(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--debug", action="store_true", help="Log more")
    parser.add_argument(
        "--fd",
        type=int,
        action="append",
        default=[],
        help="FDs will be provided to the wrapped process with sequential "
        "numbers starting from 3, in the order they were listed on "
        "the command-line. Repeat to pass multiple FDs.",
    )
    parser.add_argument(
        "--sudo",
        action="store_true",
        help="Wrap `recv-fds-and-run` with `sudo`, effectively emulating the "
        "behavior of `sudo -C`. See `--sudo-arg` if you need to "
        "pass arguments to `sudo`. Without this option, this CLI is a "
        "very elaborate way of remapping the listed FDs and closing all "
        "others.",
    )
    parser.add_argument(
        "--sudo-arg",
        action="append",
        default=[],
        help="Pass this argument to `sudo` on the command-line.",
    )
    parser.add_argument(
        "--no-set-listen-fds",
        action="store_false",
        dest="set_listen_fds",
        help="Do not set LISTEN_FDS and LISTEN_PID on the wrapped process. By "
        "default we set these just in case this wraps `systemd-nspawn` -- "
        "that tells it to forward our FDS to the container. If the extra "
        "environment variables are a problem for you, pass this option.",
    )
    parser.add_argument(
        "cmd", nargs="+", help="The command to wrap and supply with FDs."
    )
    opts = Path.parse_args(parser, argv)
    assert not opts.sudo_arg or opts.sudo, "--sudo-arg requires --sudo"
    return opts


def send_fds_and_popen(opts, **popen_kwargs):
    return popen_and_inject_fds_after_sudo(
        opts.cmd,
        opts.fd,
        lambda wrapped_cmd: subprocess.Popen(
            [
                *(["sudo", *opts.sudo_arg, "--"] if opts.sudo else []),
                *wrapped_cmd,
            ],
            **popen_kwargs,
        ),
        set_listen_fds=opts.set_listen_fds,
    )


# The CLI functionality is pretty well-covered in `test_send_fds_and_run.py`.
# Here is a manual smoke test that should print nothing and exit with code 1.
#     buck run //antlir:send-fds-and-run -- --no-set-listen-fds -- \
#         printenv LISTEN_FDS LISTEN_PID ; echo $?
if __name__ == "__main__":  # pragma: no cover
    opts = parse_opts(sys.argv[1:])
    init_logging(debug=opts.debug)
    with send_fds_and_popen(opts) as proc:
        # Since this program is a wrapper, it ought not keep random other
        # FDs open.  The harm of leaving them open is that the parties using
        # the FDs to communicate might want to wait for a stream to get
        # closed ...  which would never happen, causing a deadlock.
        for fd in opts.fd:
            # NB: This declines to close stderr since that would break any
            # future attempts to log from this code (afaik, there are not
            # any at present).  This isn't a true FD leak, since `sudo`
            # would also keep FD 2 open, anyway.  This wrapper never has any
            # business writing to 1, or reading from 0, so close those.
            if fd != 2:
                os.close(fd)
    sys.exit(proc.returncode)
