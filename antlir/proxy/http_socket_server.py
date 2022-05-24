# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import socket
from socketserver import BaseServer


class HTTPSocketServer(BaseServer):
    """
    A lightweight clone of the built-in HTTPServer & TCPServer to work
    around the fact that they do not accept pre-existing sockets.
    """

    # pyre-fixme[2]: Parameter must be annotated.
    def __init__(self, sock: socket.socket, RequestHandlerClass) -> None:
        """
        We just listen on `sock`. It may or may not be bound to any host or
        port **yet** -- and in fact, the binding will be done by another
        process on our behalf.
        """
        # No server address since nothing actually needs to know it.
        super().__init__(None, RequestHandlerClass)
        self.socket = sock

    # This is only here as part of the BaseServer API, never to be run.
    # pyre-fixme[3]: Return type must be annotated.
    def server_bind(self):  # pragma: no cover
        raise AssertionError(
            "self.socket must be bound externally before self.server_activate"
        )

    def server_activate(self) -> None:
        self.socket.listen()  # leave the request queue size at default

    def server_close(self) -> None:
        self.socket.close()

    def fileno(self) -> int:
        return self.socket.fileno()

    # pyre-fixme[3]: Return type must be annotated.
    def get_request(self):
        return self.socket.accept()

    # pyre-fixme[2]: Parameter must be annotated.
    def shutdown_request(self, request) -> None:
        try:
            # Explicitly shutdown -- `socket.close()` merely releases the
            # socket and waits for GC to perform the actual close.
            request.shutdown(socket.SHUT_WR)
        # This is cribbed from the Python standard library, but I have no
        # idea how to test it, hence the pragma.
        except OSError:  # pragma: no cover
            pass  # Some platforms may raise ENOTCONN here
        self.close_request(request)

    # pyre-fixme[2]: Parameter must be annotated.
    def close_request(self, request) -> None:
        request.close()
