# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import os
import subprocess
import unittest


class Test(unittest.TestCase):
    def test_run(self) -> None:
        with importlib.resources.path(__package__, "test.xar") as path:
            proc = subprocess.run([path], check=True, text=True, capture_output=True)
        self.assertEqual(proc.stdout, "foo\n")

    def test_is_executable(self) -> None:
        with importlib.resources.path(__package__, "test.xar") as path:
            self.assertTrue(os.access(path, os.X_OK))
