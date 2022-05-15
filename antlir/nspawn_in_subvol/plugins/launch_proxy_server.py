#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import socket
import subprocess
from contextlib import contextmanager, ExitStack
from typing import Generator

from antlir.common import get_logger
from antlir.fs_utils import Path

from .server_launcher import ServerLauncher

PROXY_SERVER_PORT = 45063


log = get_logger()
_mockable_popen_for_repo_server = subprocess.Popen


class ProxyServer(ServerLauncher):
    def __init__(self, sock: socket.socket, fbpkg_db_path: Path) -> None:
        bin_path = ""
        with Path.resource(__package__, "proxy-server", exe=True) as p:
            bin_path = p

        self.fbpkg_db_path = fbpkg_db_path
        if not bin_path:  # pragma: no cover
            raise RuntimeError("proxy-server file could not be found.")
        super().__init__(port=PROXY_SERVER_PORT, sock=sock, bin_path=bin_path)

    def __format__(self, format_spec: str) -> str:
        return f"ProxyServer(port={self.port})"

    @property
    def command_line(self):
        return [
            self.bin_path,
            f"--socket-fd={self.sock.fileno()}",
            f"--fbpkg-db-path={self.fbpkg_db_path}",
            *(["--debug"] if log.isEnabledFor(logging.DEBUG) else []),
        ]


@contextmanager
def launch_proxy_server_for_netns(
    *, ns_socket: socket.socket, fbpkg_db_path: Path
) -> Generator[ProxyServer, None, None]:
    """
    Yields ProxyServer object where the server will listen.
    """

    with ExitStack() as stack:
        proxy_server = ProxyServer(sock=ns_socket, fbpkg_db_path=fbpkg_db_path)
        stack.enter_context(proxy_server.launch())
        log.debug(f"Launched {proxy_server}")
        yield proxy_server
