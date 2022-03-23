#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import signal
import socket
import subprocess
from contextlib import ExitStack, contextmanager
from typing import List, NamedTuple, Optional

from antlir.common import check_popen_returncode, get_logger
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.netns_socket import create_sockets_inside_netns


log = get_logger()
_mockable_popen_for_repo_server = subprocess.Popen


class RepoServer(NamedTuple):
    rpm_repo_snapshot: Path
    port: int
    # The socket & server are invalid after the `_launch_repo_server` context
    sock: socket.socket
    proc: Optional[subprocess.Popen] = None

    # pyre-fixme[14]: `__format__` overrides method defined in `object` inconsistently.
    def __format__(self, _spec) -> str:
        return f"RepoServer({self.rpm_repo_snapshot}, port={self.port})"


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
    # interactive `=container` boot time by hundreds of ms.
    rs.sock.listen()  # leave the request queue size at default
    with rs.sock, _mockable_popen_for_repo_server(
        [
            repo_server_bin,
            f"--socket-fd={rs.sock.fileno()}",
            # TODO: Once the committed BAs all have a `repo-server` that
            # knows to append `/snapshot` to the path, remove it here, and
            # tidy up the snapshot resolution code in `repo_server.py`.
            f"--snapshot-dir={rs.rpm_repo_snapshot / 'snapshot'}",
            *(["--debug"] if log.isEnabledFor(logging.DEBUG) else []),
        ],
        pass_fds=[rs.sock.fileno()],
    ) as server_proc:
        try:
            # pyre-fixme[7]: Expected `RepoServer` but got
            # `Generator[RepoServer, None, None]`.
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
            create_sockets_inside_netns(target_pid, len(repo_server_ports)),
            repo_server_ports,
        ):
            rs = stack.enter_context(
                # pyre-fixme[6]: Expected `ContextManager[Variable[
                # contextlib._T]]` for 1st param but got `RepoServer`.
                _launch_repo_server(
                    repo_server_bin,
                    RepoServer(
                        rpm_repo_snapshot=snapshot_dir,
                        port=port,
                        sock=sock,
                    ),
                )
            )
            log.debug(f"Launched {rs} in {target_pid}'s netns")
            servers.append(rs)
        # pyre-fixme[7]: Expected `List[RepoServer]` but got
        #  `Generator[List[typing.Any], None, None]`.
        yield servers
