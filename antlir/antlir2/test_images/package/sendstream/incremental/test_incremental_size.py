# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import os.path
import unittest


class TestIncrementalSize(unittest.TestCase):
    def _test_size(self, parent_resource, child_resource) -> None:
        with importlib.resources.path(__package__, child_resource) as child:
            child = os.path.getsize(child)
        # The full sendstream would be well over 100Mb. The incremental only
        # logically added 10MB, but there is a bit of overhead in the
        # incremental stream format
        self.assertAlmostEqual(child, 10 * 1024 * 1024, delta=8192)

        # Also just make sure the parent sendstream is big (setting the minimum
        # at 80MB, but it's actually >100MB at the time of this writing)
        with importlib.resources.path(__package__, parent_resource) as parent:
            parent = os.path.getsize(parent)
        self.assertGreater(parent, 80 * 1024 * 1024)

    def test_incremental_size(self) -> None:
        self._test_size("parent.sendstream", "child.sendstream")

    def test_incremental_size_rootless(self) -> None:
        self._test_size("parent.sendstream.rootless", "child.sendstream.rootless")
