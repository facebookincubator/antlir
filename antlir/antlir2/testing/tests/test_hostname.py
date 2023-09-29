# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import socket
import unittest


class TestHostname(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_hostname(self) -> None:
        self.assertEqual(socket.gethostname(), "antlir2-test-hostname")
