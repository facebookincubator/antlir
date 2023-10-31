# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from importlib.resources import read_text


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_configured_alias(self) -> None:
        for variant in ["centos8", "centos9", "default"]:
            with self.subTest(variant):
                self.assertEqual(variant + "\n", read_text(__package__, "f." + variant))
