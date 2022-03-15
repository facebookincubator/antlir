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
    def do_GET(self):
        data = bytes(json.dumps({"test": "data"}), "ascii")
        if self.path == "/error":
            self.send_response(HTTPStatus.INTERNAL_SERVER_ERROR)
        else:
            self.send_response(HTTPStatus.OK)

        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


class TestProxyURL(TestCase):
    def setUp(self):
        self.path_to_cert = None
        with importlib.resources.path(__package__, "test-cert") as p:
            self.path_to_cert = p / "localhost.pem"

        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.bind(("127.0.0.1", 0))
        self.port = sock.getsockname()[1]
        sock.close()

        self.server = HTTPServer(("127.0.0.1", self.port), TestHandler)
        self.server.socket = ssl.wrap_socket(
            self.server.socket, certfile=self.path_to_cert, server_side=True
        )

        server_thread = Thread(target=self.server.serve_forever)
        server_thread.setDaemon(True)
        server_thread.start()

    def tearDown(self):
        self.server.server_close

    def test_proxy_url(self):
        handler = MagicMock()
        handler.send_error = MagicMock()
        handler.send_response = MagicMock()
        handler.wfile = BytesIO()

        context = ssl.create_default_context()

        context.load_verify_locations(cafile=self.path_to_cert)
        proxy_url(f"https://localhost:{self.port}", handler, context=context)

        handler.send_response.assert_called_once_with(HTTPStatus.OK)
        self.assertEqual(
            handler.wfile.getvalue(),
            bytes(json.dumps({"test": "data"}), "ascii"),
        )

    def test_proxy_url_error(self):
        handler = MagicMock()
        handler.send_error = MagicMock()
        handler.send_response = MagicMock()
        handler.wfile = BytesIO()

        context = ssl.create_default_context()
        context.load_verify_locations(cafile=self.path_to_cert)
        proxy_url(
            f"https://localhost:{self.port}/error", handler, context=context
        )

        handler.send_response.assert_called_once_with(
            HTTPStatus.INTERNAL_SERVER_ERROR
        )

    def test_proxy_url_http(self):
        handler = MagicMock()
        handler.send_error = MagicMock()
        handler.send_response = MagicMock()
        handler.wfile = BytesIO()

        proxy_url(
            f"http://localhost:{self.port}/", handler, allow_insecure_http=True
        )
        handler.send_response.assert_not_called()
        handler.send_error.assert_called_once_with(
            HTTPStatus.INTERNAL_SERVER_ERROR,
            "[Errno 104] Connection reset by peer",
        )
