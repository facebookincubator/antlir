#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import socket
import subprocess
from contextlib import contextmanager, ExitStack
from typing import Generator, List

from antlir.common import get_logger
from antlir.fs_utils import Path

from antlir.nspawn_in_subvol.plugins.server_launcher import ServerLauncher


log = get_logger()
_mockable_popen_for_repo_server = subprocess.Popen


class RepoServer(ServerLauncher):
    def __init__(self, rpm_repo_snapshot: Path, **kwargs) -> None:
        super().__init__(**kwargs)
        self.rpm_repo_snapshot = rpm_repo_snapshot

    def __format__(self, format_spec: str) -> str:
        return f"RepoServer({self.rpm_repo_snapshot}, port={self.port})"

    @property
    def command_line(self):
        return [
            self.bin_path,
            f"--socket-fd={self.sock.fileno()}",
            # TODO: Once the committed BAs all have a `repo-server` that
            # knows to append `/snapshot` to the path, remove it here, and
            # tidy up the snapshot resolution code in `repo_server.py`.
            f"--snapshot-dir={self.rpm_repo_snapshot / 'snapshot'}",
            *(["--debug"] if log.isEnabledFor(logging.DEBUG) else []),
        ]


@contextmanager
def launch_repo_servers_for_netns(
    *,
    ns_sockets: List[socket.socket],
    snapshot_dir: Path,
    repo_server_bin: Path,
) -> Generator[List[RepoServer], None, None]:
    """
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
            ns_sockets,
            repo_server_ports,
        ):
            repo_server = RepoServer(
                rpm_repo_snapshot=snapshot_dir,
                port=port,
                sock=sock,
                bin_path=repo_server_bin,
            )

            rs = stack.enter_context(repo_server.launch())
            log.debug(f"Launched {rs}")
            servers.append(rs)

        yield servers
