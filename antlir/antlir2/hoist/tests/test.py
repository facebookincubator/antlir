# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import os
import stat
import unittest
from pathlib import Path


class Test(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def _test_single_file(self, path: Path, basename: str = "f") -> None:
        self.assertTrue(path.exists())
        self.assertEqual(path.read_text(), "single file\n")
        self.assertEqual(path.stat().st_mode & 0o777, 0o444)
        self.assertEqual(path.stat().st_uid, os.getuid())
        self.assertEqual(path.stat().st_gid, os.getgid())
        self.assertEqual(path.name, basename)

    def test_single_file_rootless(self) -> None:
        self._test_single_file(Path(os.getenv("SINGLE_FILE_ROOTLESS")))

    def test_single_file_rooted(self) -> None:
        self._test_single_file(Path(os.getenv("SINGLE_FILE_ROOTED")))

    def test_single_file_from_multi_paths(self) -> None:
        self._test_single_file(
            Path(os.getenv("MULTIPLE_PATHS_SINGLE_FILE")), basename="file"
        )

    def test_executable(self) -> None:
        path = Path(os.getenv("EXECUTABLE"))
        self.assertEqual(path.stat().st_mode & stat.S_IXUSR, stat.S_IXUSR)

    def _test_dir(self, path: Path) -> None:
        self.assertEqual(path.stat().st_mode & 0o777, 0o755)
        self.assertEqual(path.stat().st_uid, os.getuid())
        self.assertEqual(path.stat().st_gid, os.getgid())

        self.assertEqual((path / "file1").read_text(), "multi file 1\n")
        self.assertTrue((path / "nested").is_dir())
        self.assertTrue((path / "nested/sh").is_file())

    def test_dir(self) -> None:
        path = Path(os.getenv("HOISTED_DIR"))
        self._test_dir(path)

    def test_projected(self) -> None:
        root = Path(os.getenv("PROJECTED_MULTI"))
        self._test_dir(root)

        path = Path(os.getenv("PROJECTED_MULTI_NESTED"))
        self.assertTrue(path.is_dir())
        self.assertTrue((path / "sh").exists())
        self.assertEqual(path, root / "nested")

        path = Path(os.getenv("PROJECTED_MULTI_NESTED_SH"))
        self.assertEqual(path.stat().st_mode & stat.S_IXUSR, stat.S_IXUSR)
