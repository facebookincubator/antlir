#!/usr/bin/env python3
import os
import unittest


class UnsanitizedEnvTest(unittest.TestCase):

    def test_env(self):
        # Comes from Buck
        self.assertIn('BUCK_BUILD_ID', os.environ)
        # Comes from the test's `env`
        self.assertEqual('meow', os.environ['kitteh'])
