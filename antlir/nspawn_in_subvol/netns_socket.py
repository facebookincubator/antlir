# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import socket
import subprocess
import textwrap
import time
from typing import List

from antlir.common import (
    check_popen_returncode,
    FD_UNIX_SOCK_TIMEOUT,
    get_logger,
    listen_temporary_unix_socket,
    recv_fds_from_unix_sock,
)

# pyre-fixme[5]: Global expression must be annotated.
log = get_logger()
# pyre-fixme[24]: Generic type `subprocess.Popen` expects 1 type parameter.
_mockable_popen = subprocess.Popen


# pyre-fixme[2]: Parameter must be annotated.
def _make_debug_print(logger_name, fstring) -> str:
    t = time.time()
    ymdhms = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(t))
    msecs = int((t - int(t)) * 1000)
    return (
        "print("
        # Emulate the format of `init_logging(debug=True)`
        + repr(f"DEBUG _make_sockets_and_send_via {ymdhms},{msecs:03} ")
        + " + f'Sending {num_socks} FDs to parent', file=sys.stderr)"
    )


# pyre-fixme[3]: Return type must be annotated.
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


def create_sockets_inside_netns(
    target_pid: int, num_socks: int
) -> List[socket.socket]:
    """
    Creates requested number of TCP stream sockets inside the container.

    Returns the socket.socket() object.
    """
    with listen_temporary_unix_socket() as (
        unix_sock_path,
        list_sock,
    ), _mockable_popen(
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
        server_socks = [
            socket.socket(fileno=fd)
            for fd in recv_fds_from_unix_sock(unix_sock_path, num_socks)
        ]

        assert len(server_socks) == num_socks, len(server_socks)
    check_popen_returncode(sock_proc)
    return server_socks
