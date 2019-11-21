#!/usr/bin/env python3
import os
import unittest

from find_built_subvol import find_built_subvol


class FindBuiltSubvolTestCase(unittest.TestCase):
    def test_find_built_subvol(self):
        with open(find_built_subvol(
            # This works in @mode/opt since this artifact is baked into the XAR
            os.path.join(os.path.dirname(__file__), 'hello_world_base'),
        ).path('hello_world')) as f:
            self.assertEqual('', f.read())
