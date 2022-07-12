#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import sys
import unittest

from antlir.fs_utils import temp_dir

from antlir.rpm.open_url import open_url


class OpenUrlTestCase(unittest.TestCase):
    def test_open_http_url(self) -> None:
        with temp_dir() as server_dir:
            hello_path = server_dir / "hello"
            with open(hello_path, "w") as out_f:
                out_f.write("world")

            # First, check file:// URLs
            with open_url(hello_path.file_url()) as in_f:
                self.assertEqual(b"world", in_f.read())

            # Now, http:// URLs
            with subprocess.Popen(
                [
                    sys.executable,
                    "-c",
                    """
import http.server as hs
with hs.HTTPServer(('localhost', 0), hs.SimpleHTTPRequestHandler) as httpd:
    print('http://{}:{}/'.format(*httpd.socket.getsockname()), flush=True)
    httpd.serve_forever()
                """,
                ],
                cwd=server_dir,
                stdout=subprocess.PIPE,
            ) as proc:
                try:
                    with open_url(
                        # pyre-fixme[16]: Optional type has no attribute `readline`.
                        proc.stdout.readline().decode().rstrip("\n")
                        + "hello"
                    ) as in_f:
                        self.assertEqual(b"world", in_f.read())
                finally:
                    proc.kill()
