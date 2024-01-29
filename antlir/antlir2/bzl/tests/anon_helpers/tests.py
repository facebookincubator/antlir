# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import json
import unittest


class Tests(unittest.TestCase):
    def test_defaults(self) -> None:
        out = json.loads(importlib.resources.read_text(__package__, "defaults"))
        self.assertEqual(out["int"], 42)
        self.assertEqual(out["str"], "hello")

    def test_override(self) -> None:
        out = json.loads(importlib.resources.read_text(__package__, "override"))
        self.assertEqual(out["cpu"], "foo")
        self.assertEqual(out["int"], 42)
        self.assertEqual(out["str"], "hello")
