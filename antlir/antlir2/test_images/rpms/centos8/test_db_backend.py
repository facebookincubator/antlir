# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest
from pathlib import Path


class TestDbBackend(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_backend_is_ndb(self) -> None:
        self.assertEqual(
            subprocess.run(
                ["rpm", "-E", "%{_db_backend}"],
                check=True,
                text=True,
                capture_output=True,
            ).stdout.strip(),
            "ndb",
        )

    def test_db_files(self) -> None:
        db_path = Path(
            subprocess.run(
                ["rpm", "-E", "%{_dbpath}"], check=True, text=True, capture_output=True
            ).stdout.strip()
        )
        self.assertTrue((db_path / "Packages.db").exists())
        self.assertFalse((db_path / "rpmdb.sqlite").exists())
        # there are more, but just check for a few and that's good enough
        for f in {"Packages", "Dirnames", "Group", "Name"}:
            with self.subTest(f"dbd file {f}"):
                self.assertFalse((db_path / f).exists())
