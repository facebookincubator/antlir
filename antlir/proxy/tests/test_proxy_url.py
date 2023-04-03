# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import json
import socket
import ssl
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from io import BytesIO
from threading import Thread
from unittest import TestCase
from unittest.mock import MagicMock

from antlir.proxy.proxy_url import proxy_url


class TestHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        data = bytes(json.dumps({"test": "data"}), "ascii")
        if self.path == "/error":
            self.send_response(HTTPStatus.INTERNAL_SERVER_ERROR)
        else:
            self.send_response(HTTPStatus.OK)

        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def _mock_handler():
    handler = MagicMock()
    handler.send_error = MagicMock()
    handler.send_response = MagicMock()
    handler.wfile = BytesIO()
    return handler


class TestProxyURL(TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.path_to_cert = None
        with importlib.resources.path(__package__, "test-cert") as p:
            cls.path_to_cert = p / "localhost.pem"

        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.bind(("127.0.0.1", 0))
        cls.port = sock.getsockname()[1]
        sock.close()

        cls.server = HTTPServer(("127.0.0.1", cls.port), TestHandler)
        cls.server.socket = ssl.wrap_socket(
            cls.server.socket, certfile=cls.path_to_cert, server_side=True
        )

        cls.ssl_context = ssl.create_default_context(ssl.Purpose.SERVER_AUTH)
        cls.ssl_context.load_verify_locations(cafile=cls.path_to_cert)

        server_thread = Thread(target=cls.server.serve_forever)
        server_thread.daemon = True
        server_thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.server_close()

    def test_proxy_url(self) -> None:
        handler = _mock_handler()

        proxy_url(f"https://localhost:{self.port}", handler, context=self.ssl_context)

        handler.send_response.assert_called_once_with(HTTPStatus.OK)
        self.assertEqual(
            handler.wfile.getvalue(),
            bytes(json.dumps({"test": "data"}), "ascii"),
        )

    def test_proxy_url_error(self) -> None:
        handler = _mock_handler()

        proxy_url(
            f"https://localhost:{self.port}/error",
            handler,
            context=self.ssl_context,
        )

        handler.send_response.assert_called_once_with(HTTPStatus.INTERNAL_SERVER_ERROR)

    def test_proxy_url_http(self) -> None:
        handler = _mock_handler()

        proxy_url(f"http://localhost:{self.port}/", handler, allow_insecure_http=True)
        handler.send_response.assert_not_called()
        handler.send_error.assert_called_once_with(
            HTTPStatus.INTERNAL_SERVER_ERROR,
            "[Errno 104] Connection reset by peer",
        )
