# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import signal
import socket
import subprocess
from contextlib import contextmanager
from typing import Generator, List

from antlir.common import check_popen_returncode, get_logger
from antlir.fs_utils import Path


log = get_logger()
_mockable_popen_for_server = subprocess.Popen


class ServerLauncher:
    """
    An abstract parent class for socket servers that need to be launched
    to serve sockets in namespaces
    """

    def __init__(self, port: int, sock: socket.socket, bin_path: Path) -> None:
        self.port = port
        self.sock = sock
        self.bin_path = bin_path

    @property
    def command_line(self) -> List[str]:  # pragma: no cover
        raise RuntimeError("Child class has to implement this.")

    @contextmanager
    def launch(self) -> Generator[object, None, None]:
        """
        Launches a server and returns it.
        """
        self.sock.bind(("127.0.0.1", self.port))
        # Socket activation: allow requests to queue up, which means that
        # we don't have to explicitly wait for the servers to start --
        # any in-container clients will do so if/when needed. This reduces
        # interactive `=container` boot time by hundreds of ms.
        self.sock.listen()  # leave the request queue size at default
        with self.sock, _mockable_popen_for_server(
            self.command_line,
            pass_fds=[self.sock.fileno()],
        ) as server_proc:
            try:
                yield self
            finally:
                # Uh-oh, the server already exited. Did it crash?
                if server_proc.poll() is not None:  # pragma: no cover
                    check_popen_returncode(server_proc)
                else:
                    # Although these servers are a read-only proxy, give it the
                    # chance to do graceful cleanup.
                    log.debug("Trying to gracefully terminate server")
                    server_proc.send_signal(signal.SIGTERM)
                    try:
                        server_proc.wait(60.0)
                    except subprocess.TimeoutExpired:  # pragma: no cover
                        log.warning(f"Killing unresponsive server {server_proc.pid}")
                        server_proc.kill()
