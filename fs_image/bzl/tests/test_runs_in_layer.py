#!/usr/bin/env python3
import os
import unittest

from coverage_test_helper import coverage_test_helper


class RunsInLayerTest(unittest.TestCase):

    def test_unique_path_exists(self):
        # This should cause our 100% coverage assertion to pass.
        coverage_test_helper()
        # Ensure that the containers are running inside the correct layer
        self.assertTrue(os.path.exists("/unique/test/path"))
