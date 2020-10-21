#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import signal
import socket
import subprocess
import textwrap
import time
from contextlib import ExitStack, contextmanager
from typing import List, NamedTuple, Optional

from antlir.common import (
    FD_UNIX_SOCK_TIMEOUT,
    check_popen_returncode,
    get_logger,
    listen_temporary_unix_socket,
    recv_fds_from_unix_sock,
)
from antlir.fs_utils import Path


log = get_logger()
_mockable_popen_for_repo_server = subprocess.Popen


def _make_debug_print(logger_name, fstring):
    t = time.time()
    ymdhms = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(t))
    msecs = int((t - int(t)) * 1000)
    return (
        "print("
        # Emulate the format of `init_logging(debug=True)`
        + repr(f"DEBUG _make_sockets_and_send_via {ymdhms},{msecs:03} ")
        + " + f'Sending {num_socks} FDs to parent', file=sys.stderr)"
    )


def _make_sockets_and_send_via(*, num_socks: int, unix_sock_fd: int):
    """
    Creates a TCP stream socket and sends it elsewhere via the provided Unix
    domain socket file descriptor.  This is useful for obtaining a socket
    that belongs to a different network namespace (i.e.  creating a socket
    inside a container, but binding it from outside the container).

    IMPORTANT: This code must not write anything to stdout, the fd can be 1.
    """

    # NB: Some code here is (sort of) copy-pasta'd in `send_fds_and_run.py`,
    # but it's not obviously worthwhile to reuse it here.
    return [
        "python3",
        "-c",
        textwrap.dedent(
            """
    import array, contextlib, socket, sys

    def send_fds(sock, msg: bytes, fds: 'List[int]'):
        num_sent = sock.sendmsg([msg], [(
            socket.SOL_SOCKET, socket.SCM_RIGHTS,
            array.array('i', fds).tobytes(),
            # Future: is `flags=socket.MSG_NOSIGNAL` a good idea?
        )])
        assert len(msg) == num_sent, (msg, num_sent)

    num_socks = """
            + str(num_socks)
            + """
    """  # indentation for the debug print
            + (
                _make_debug_print(
                    "_make_sockets_and_send_via",
                    "f'Sending {num_socks} FDs to parent'",
                )
                if log.isEnabledFor(logging.DEBUG)
                else ""
            )
            + """
    with contextlib.ExitStack() as stack:
        # Make a socket in this netns, and send it to the parent.
        lsock = stack.enter_context(
            socket.socket(fileno="""
            + str(unix_sock_fd)
            + """)
        )
        lsock.settimeout("""
            + str(FD_UNIX_SOCK_TIMEOUT)
            + """)

        csock = stack.enter_context(lsock.accept()[0])
        csock.settimeout("""
            + str(FD_UNIX_SOCK_TIMEOUT)
            + """)

        send_fds(csock, b'ohai', [
            stack.enter_context(socket.socket(
                socket.AF_INET, socket.SOCK_STREAM
            )).fileno()
                for _ in range(num_socks)
        ])
    """
        ),
    ]


def _create_sockets_inside_netns(
    target_pid: int, num_socks: int
) -> List[socket.socket]:
    """
    Creates TCP stream socket inside the container.

    Returns the socket.socket() object.
    """
    with listen_temporary_unix_socket() as (
        unix_sock_path,
        list_sock,
    ), subprocess.Popen(
        [
            # NB: /usr/local/fbcode/bin must come first because /bin/python3
            # may be very outdated
            "sudo",
            "env",
            "PATH=/usr/local/fbcode/bin:/bin",
            "nsenter",
            "--net",
            "--target",
            str(target_pid),
            # NB: We pass our listening socket as FD 1 to avoid dealing with
            # the `sudo` option of `-C`.  Nothing here writes to `stdout`:
            *_make_sockets_and_send_via(unix_sock_fd=1, num_socks=num_socks),
        ],
        stdout=list_sock.fileno(),
    ) as sock_proc:
        repo_server_socks = [
            socket.socket(fileno=fd)
            for fd in recv_fds_from_unix_sock(unix_sock_path, num_socks)
        ]
        assert len(repo_server_socks) == num_socks, len(repo_server_socks)
    check_popen_returncode(sock_proc)
    return repo_server_socks


class RepoServer(NamedTuple):
    snapshot_dir: Path
    port: int
    # The socket & server are invalid after the `_launch_repo_server` context
    sock: socket.socket
    proc: Optional[subprocess.Popen] = None

    def __format__(self, _spec):
        return f"RepoServer({self.snapshot_dir}, port={self.port})"


@contextmanager
def _launch_repo_server(repo_server_bin: Path, rs: RepoServer) -> RepoServer:
    """
    Invokes `repo-server` with the given snapshot; passes it ownership of
    the bound TCP socket -- it listens & accepts connections.

    Returns a copy of the `RepoServer` with `server` populated.
    """
    assert rs.proc is None
    rs.sock.bind(("127.0.0.1", rs.port))
    # Socket activation: allow requests to queue up, which means that
    # we don't have to explicitly wait for the repo servers to start --
    # any in-container clients will do so if/when needed. This reduces
    # interactive `-container` boot time by hundreds of ms.
    rs.sock.listen()  # leave the request queue size at default
    with rs.sock, _mockable_popen_for_repo_server(
        [
            repo_server_bin,
            "--socket-fd",
            str(rs.sock.fileno()),
            "--snapshot-dir",
            rs.snapshot_dir,
            *(["--debug"] if log.isEnabledFor(logging.DEBUG) else []),
        ],
        pass_fds=[rs.sock.fileno()],
    ) as server_proc:
        try:
            yield rs._replace(proc=server_proc)
        finally:
            # Uh-oh, the server already exited. Did it crash?
            if server_proc.poll() is not None:  # pragma: no cover
                check_popen_returncode(server_proc)
            else:
                # Although `repo-server` is a read-only proxy, give it the
                # chance to do graceful cleanup.
                log.debug("Trying to gracefully terminate `repo-server`")
                # `atexit` (used in an FB-specific `repo-server` plugin) only
                # works on graceful termination.  In `repo_server_main.py`, we
                # graceful set up handling of `SIGTERM`.  We signal once, and
                # need to wait for it to clean up the resources it must to free.
                # Signaling twice would interrupt cleanup (because this is
                # Python, lol).
                server_proc.send_signal(signal.SIGTERM)
                try:
                    server_proc.wait(60.0)
                except subprocess.TimeoutExpired:  # pragma: no cover
                    log.warning(
                        f"Killing unresponsive `repo-server` {server_proc.pid}"
                    )
                    server_proc.kill()


@contextmanager
def launch_repo_servers_for_netns(
    *, target_pid: int, snapshot_dir: Path, repo_server_bin: Path
) -> List[RepoServer]:
    """
    Creates sockets inside the supplied netns, and binds them to the
    supplied ports on localhost.

    Yields a list of (host, port) pairs where the servers will listen.
    """
    with open(snapshot_dir / "ports-for-repo-server") as infile:
        repo_server_ports = {int(v) for v in infile.read().split() if v}
    with ExitStack() as stack:
        # Start a repo-server instance per port.  Give each one a socket
        # bound to the loopback inside the supplied netns.  We don't
        # `__enter__` the sockets since the servers take ownership of them.
        servers = []
        for sock, port in zip(
            _create_sockets_inside_netns(target_pid, len(repo_server_ports)),
            repo_server_ports,
        ):
            rs = stack.enter_context(
                _launch_repo_server(
                    repo_server_bin,
                    RepoServer(
                        snapshot_dir=snapshot_dir / "snapshot",
                        port=port,
                        sock=sock,
                    ),
                )
            )
            log.debug(f"Launched {rs} in {target_pid}'s netns")
            servers.append(rs)
        yield servers
