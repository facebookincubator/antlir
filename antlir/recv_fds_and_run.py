#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
`sudo -C` is forbidden by default, making it harder to pass file descriptors
to programs executed under `sudo`.  This wrapper bypasses the issue by
receiving file descriptors over a Unix domain socket, as sent via the
library / binary implemented in `send_fds_and_run.py`.  Here is all it does:

  - Connects to `--unix-sock` and received `--num-fds`.
  - Keeps FDs 0, 1, 2 as-is.
  - Assigns received FDs sequentially from 3 onwards.
  - Closes every other FD.
  - Optionally sets environment variables to tell `systemd-nspawn` to do
    forward the same FDs into the container.
  - Calls `execvpe` to start the wrapped process.

Refer to the `send_fds_and_run.py` docblock for a map of how this helper
integrates with the chain of process calls used in a typical `sudo`
invocation.
"""
import argparse
import os
import resource
import sys

from antlir.common import get_logger, init_logging, recv_fds_from_unix_sock
from antlir.fs_utils import Path


log = get_logger()


def parse_opts(argv):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--debug", action="store_true", help="Log more")
    parser.add_argument(
        "--unix-sock",
        required=True,
        help="Connect to the unix socket at this path to receive FDs.",
    )
    parser.add_argument(
        "--num-fds",
        type=int,
        required=True,
        help="The number of FDs to inject, from 3 through (3 + NUM_FDS - 1).",
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
    return Path.parse_args(parser, argv)


def main() -> None:  # pragma: no cover
    opts = parse_opts(sys.argv[1:])
    init_logging(debug=opts.debug)

    log.debug(f"Receiving {opts.num_fds} FDs via {opts.unix_sock}")
    fds = recv_fds_from_unix_sock(opts.unix_sock, max_fds=opts.num_fds)
    assert len(fds) == opts.num_fds, (fds, opts)
    max_fd_count = max(resource.getrlimit(resource.RLIMIT_NOFILE))
    max_set_fd = 2 + len(fds)
    assert not fds or min(fds) >= 3, f"Some FD was < 3 in {fds}"
    assert len(set(fds)) == len(fds), f"Not all FDs {fds} were distinct"
    # We will `dup2` FDs from the smallest target to the greatest.  Our
    # smallest target is always 3, our smallest source is at LEAST 3 by the
    # assertion above, so the first `dup2` is safe.  After the first `dup2`,
    # our smallest target will be 4.  By uniqueness (asserted above), the
    # smallest source now has to be at least 4, making the second `dup2`
    # safe also.  This is an inductive proof that our `dup2`s never clobber
    # a received FD in the process of re-mapping them.
    fd_map = list(zip(fds, range(3, max_set_fd + 1)))
    log.debug(f"Passing received FDs as {dict(fd_map)} into {opts.cmd}")
    env = os.environ.copy()
    if opts.set_listen_fds:
        env["LISTEN_PID"] = str(os.getpid())
        env["LISTEN_FDS"] = str(len(fds))

    # IMPORTANT: No more file descriptor operations are permitted from
    # here on onwards, since we are clobbering our own FD table!
    # NB: Prior logging should be unaffected, since it must flush stderr.
    for src_fd, target_fd in fd_map:
        os.dup2(src_fd, target_fd)
    os.closerange(max_set_fd + 1, max_fd_count)
    os.execvpe(opts.cmd[0], opts.cmd, env)


# This cannot be tested as a library since it `exec`s and rewrites the file
# descriptor table.  However, `test_send_fds_and_run.py` covers this fully.
if __name__ == "__main__":
    main()  # pragma: no cover
