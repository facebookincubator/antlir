# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import unittest


LARGE_FILE_SIZE = 256 * 1024 * 1024


class Test(unittest.TestCase):
    def test_default_is_minimally_sized(self) -> None:
        with importlib.resources.path(__package__, "default.ext3") as path:
            self.assertAlmostEqual(
                path.stat().st_size,
                LARGE_FILE_SIZE,
                # why is this so off? no idea
                delta=50 * 1024 * 1024,
            )

    def test_free_mb(self) -> None:
        with importlib.resources.path(__package__, "default.ext3") as path:
            default_size = path.stat().st_size
        with importlib.resources.path(__package__, "free_mb.ext3") as path:
            self.assertAlmostEqual(
                path.stat().st_size, default_size + (256 * 1024 * 1024), delta=10 * 1024
            )

    def test_size_mb(self) -> None:
        with importlib.resources.path(__package__, "size_mb.ext3") as path:
            self.assertEqual(path.stat().st_size, 1024 * 1024 * 1024)

    def test_empty(self) -> None:
        with importlib.resources.path(__package__, "empty.ext3") as path:
            self.assertLess(path.stat().st_size, 2 * 1024 * 1024)
