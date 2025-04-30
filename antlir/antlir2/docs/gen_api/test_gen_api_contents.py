#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest


class TestGenApiContents(unittest.TestCase):
    def _gen(self) -> str:
        try:
            return subprocess.run(
                [os.environ["GEN_API"]], check=True, text=True, capture_output=True
            ).stdout
        except subprocess.CalledProcessError as e:
            raise RuntimeError(e.stderr) from e

    def test_feature_docs(self):
        self.assertIn("Install a file or directory into the image.", self._gen())

    def test_container_docs(self):
        self.assertIn("Main command to run in the job.", self._gen())
