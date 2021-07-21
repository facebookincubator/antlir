# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from typing import Optional
from unittest import mock

from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes

from ..subvol_diff import subvol_diff


class ExtractNestedFeaturesTestCase(unittest.TestCase):
    def mock_subvol_run_as_root_and_assert_raises(
        self,
        exception,
        msg: str,
        returncode: int,
        diff_output: Optional[bytes],
    ):
        left_subvol = mock.Mock()
        left_subvol.run_as_root.return_value = mock.Mock(
            returncode=returncode, stdout=diff_output
        )
        left_subvol.path.return_value = b"mock/left"

        right_subvol = mock.Mock()
        right_subvol.path.return_value = b"mock/right"

        with self.assertRaisesRegex(exception, msg):
            list(subvol_diff(left_subvol, right_subvol))
            left_subvol.run_as_root.assert_called_once()

    def test_errors(self):
        # internal diff fails to produce output
        self.mock_subvol_run_as_root_and_assert_raises(
            RuntimeError, "diff internal error: ", -1, None
        )

        # internal differ produces an unexpected output format
        self.mock_subvol_run_as_root_and_assert_raises(
            NotImplementedError, "diff line ", 1, b"wrong diff output format"
        )

        # diff found in a file whose path isn't in either subvol
        self.mock_subvol_run_as_root_and_assert_raises(
            AssertionError,
            "Neither left nor right ",
            1,
            b"Only in fake_path: fake_file",
        )

    def test_identical_subvols(self):
        with TempSubvolumes() as tmp_subvols:
            subvol = tmp_subvols.create("tmp")
            self.assertEqual(list(subvol_diff(subvol, subvol)), [])

    def test_subvol_diff(self):
        with TempSubvolumes() as tmp_subvols:
            left_subvol = tmp_subvols.create("left")
            right_subvol = tmp_subvols.create("right")

            # test different dirs get caught
            left_subvol.run_as_root(["mkdir", "-p", left_subvol.path("foo")])
            right_subvol.run_as_root(["mkdir", "-p", right_subvol.path("bar")])

            # test files of same path with different contents get caught
            left_subvol.overwrite_path_as_root(Path("foo.txt"), "leftcontent")
            right_subvol.overwrite_path_as_root(Path("foo.txt"), "rightcontent")

            # test paths expected to differ aren't caught
            left_subvol.run_as_root(
                ["mkdir", "-p", left_subvol.path("var/lib/yum")]
            )
            right_subvol.run_as_root(
                ["mkdir", "-p", right_subvol.path("var/lib/dnf")]
            )

            self.assertEqual(
                list(subvol_diff(left_subvol, right_subvol)),
                [b"./bar", b"./foo", b"foo.txt"],
            )
