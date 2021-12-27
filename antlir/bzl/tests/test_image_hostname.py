#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import socket
import unittest


class ImagePythonUnittestTest(unittest.TestCase):
    def test_container(self):
        # Ensure the hostname configuration was propagated inside the container
        self.assertEqual("test-hostname.com", socket.gethostname())
