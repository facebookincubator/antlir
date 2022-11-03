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

from antlir.nspawn_in_subvol.plugins.server_launcher import ServerLauncher

DEB_PROXY_SERVER_PORT = 45064


log: logging.Logger = get_logger()
log = get_logger()
_mockable_popen_for_repo_server = subprocess.Popen


class AptProxyServer(ServerLauncher):
    def __init__(
        self, sock: socket.socket, manifold_bucket: str, manifold_api_key: str
    ) -> None:
        self.manifold_bucket = manifold_bucket
        self.manifold_api_key = manifold_api_key
        bin_path = ""
        with Path.resource(__package__, "apt-proxy", exe=True) as p:
            bin_path = p

        if not bin_path:  # pragma: no cover
            raise RuntimeError("apt-proxy-server file could not be found.")
        super().__init__(port=DEB_PROXY_SERVER_PORT, sock=sock, bin_path=bin_path)

    def __format__(self, format_spec: str) -> str:
        return f"DebProxyServer(port={self.port})"

    @property
    def command_line(self):
        return [
            self.bin_path,
            f"--socket-fd={self.sock.fileno()}",
            f"--manifold-bucket={self.manifold_bucket}",
            f"--manifold-api-key={self.manifold_api_key}",
        ]


@contextmanager
def launch_apt_proxy_server_for_netns(
    *,
    ns_socket: socket.socket,
    bucket_name: str,
    api_key: str,
) -> Generator[AptProxyServer, None, None]:
    """
    Yields AptProxyServer object where the server will listen.
    """

    with ExitStack() as stack:
        apt_proxy_server = AptProxyServer(
            sock=ns_socket,
            manifold_bucket=bucket_name,
            manifold_api_key=api_key,
        )
        stack.enter_context(apt_proxy_server.launch())
        log.debug(f"Launched {apt_proxy_server}")
        yield apt_proxy_server
