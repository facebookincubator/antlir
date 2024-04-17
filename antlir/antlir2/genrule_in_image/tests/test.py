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

    def test_single_file(self) -> None:
        f = Path(os.getenv("SINGLE_FILE"))
        self.assertTrue(f.exists())
        self.assertEqual(f.read_text(), "foo\nbar\n")

    def _test_dir(self, p: Path) -> None:
        self.assertTrue(p.exists())
        self.assertTrue(p.is_dir())
        self.assertEqual((p / "foo").read_text(), "foo\n")

    def test_dot_dir(self) -> None:
        self._test_dir(Path(os.getenv("DOT_DIR")))

    def test_named_dir(self) -> None:
        self._test_dir(Path(os.getenv("NAMED_DIR")))

    def test_named_outs_default(self) -> None:
        self._test_dir(Path(os.getenv("NAMED_OUTS")))

    def test_default_out(self) -> None:
        f = Path(os.getenv("DEFAULT_OUT"))
        self.assertTrue(f.exists())
        self.assertTrue(f.is_dir())
        self.assertEqual((f / "baz").read_text(), "baz\n")

    def test_buck_scratch_path(self) -> None:
        f = Path(os.getenv("BUCK_SCRATCH_PATH"))
        self.assertTrue(f.exists())
        self.assertEqual(f.read_text(), "/__genrule_in_image__/buck_scratch_path\n")
