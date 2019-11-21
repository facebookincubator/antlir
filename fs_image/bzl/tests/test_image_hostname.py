#!/usr/bin/env python3
import socket
import unittest


class ImagePythonUnittestTest(unittest.TestCase):
    def test_container(self):
        # Ensure the hostname configuration was propagated inside the container
        self.assertEqual("test-hostname.com", socket.gethostname())
