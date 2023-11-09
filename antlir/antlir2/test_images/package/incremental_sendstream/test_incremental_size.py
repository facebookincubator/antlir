# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os.path
import unittest


class TestIncrementalSize(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_incremental_size(self) -> None:
        delta = os.path.getsize("/child.sendstream")
        # The full sendstream would be well over 100Mb. The incremental only
        # logically added 10MB, but there is a bit of overhead in the
        # incremental stream format
        self.assertAlmostEqual(delta, 10 * 1024 * 1024, delta=8192)
