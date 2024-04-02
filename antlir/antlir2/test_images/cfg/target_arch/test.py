#!/usr/bin/env fbpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest
from pathlib import Path


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def arch(self) -> str:
        return os.environ["ARCH"]

    def layer(self) -> Path:
        return Path(os.environ["DATA"])

    def test_parent_arch(self) -> None:
        with open(self.layer() / "parent") as f:
            parent = f.read()
        self.assertEqual(parent, self.arch())

    def test_child_arch(self) -> None:
        with open(self.layer() / "child") as f:
            child = f.read()
        self.assertEqual(child, self.arch())
