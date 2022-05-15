# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from socket import socket
from unittest import TestCase
from unittest.mock import MagicMock, patch

from antlir.proxy.http_socket_server import HTTPSocketServer


class TestHTTPSocketServer(TestCase):
    @patch.object(socket, "accept")
    @patch.object(socket, "fileno")
    @patch.object(socket, "listen")
    def test_http_socket_server(self, l_patch, f_patch, a_patch) -> None:
        with HTTPSocketServer(socket(), None) as server:
            server.server_activate()
            l_patch.assert_called_once()

            server.fileno()
            f_patch.assert_called_once()

            server.get_request()
            a_patch.assert_called_once()

            request = MagicMock()
            request.shutdown, request.close = MagicMock(), MagicMock()

            server.shutdown_request(request)
            request.shutdown.assert_called_once()
            request.close.assert_called_once()
