#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import logging
import socket
import subprocess
from contextlib import contextmanager, ExitStack
from typing import Generator, Optional

from antlir.bzl.proxy_server_config import proxy_server_config_t

from antlir.common import get_logger
from antlir.fs_utils import Path

from antlir.nspawn_in_subvol.plugins.server_launcher import ServerLauncher

PROXY_SERVER_PORT = 45063


log: logging.Logger = get_logger()
log = get_logger()
_mockable_popen_for_repo_server = subprocess.Popen


class ProxyServer(ServerLauncher):
    def __init__(
        self,
        sock: socket.socket,
        fbpkg_db_path: Optional[Path],
        proxy_server_config: Optional[proxy_server_config_t],
    ) -> None:
        bin_path = ""
        with Path.resource(__package__, "proxy-server", exe=True) as p:
            bin_path = p

        self.fbpkg_db_path = fbpkg_db_path
        self.proxy_server_config = proxy_server_config
        if not bin_path:  # pragma: no cover
            raise RuntimeError("proxy-server file could not be found.")
        super().__init__(port=PROXY_SERVER_PORT, sock=sock, bin_path=bin_path)

    def __format__(self, format_spec: str) -> str:
        return f"ProxyServer(port={self.port})"

    @property
    def command_line(self):
        optional_args = []
        # @oss-disable
            # @oss-disable
        ):  # @oss-disable pragma: no cover
            # @oss-disable
        if log.isEnabledFor(logging.DEBUG):  # pragma: no cover
            optional_args.append("--debug")

        return [
            self.bin_path,
            f"--socket-fd={self.sock.fileno()}",
            # @oss-disable
            *(optional_args),
        ]


@contextmanager
def launch_proxy_server_for_netns(
    *,
    ns_socket: socket.socket,
    fbpkg_db_path: Optional[Path],
    proxy_server_config: Optional[proxy_server_config_t],
) -> Generator[ProxyServer, None, None]:
    """
    Yields ProxyServer object where the server will listen.
    """

    with ExitStack() as stack:
        proxy_server = ProxyServer(
            sock=ns_socket,
            fbpkg_db_path=fbpkg_db_path,
            proxy_server_config=proxy_server_config,
        )
        stack.enter_context(proxy_server.launch())
        log.debug(f"Launched {proxy_server}")
        yield proxy_server
