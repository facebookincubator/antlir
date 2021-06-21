# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import platform
import unittest
from contextlib import contextmanager

from ..fs_utils import temp_dir
from ..subvol_utils import (
    Subvol,
    volume_dir,
)


@contextmanager
def _create_temp_subvol(path):
    try:
        sv = Subvol(path)
        sv.create()
        yield sv
    finally:
        if sv._exists:
            sv.delete()


class InnerSubvolTestCase(unittest.TestCase):
    def test_delete_inner_subvols(self):
        # This branch is used for testing inside an image via the
        # `:test-subvol-utils-inner` test. The hostname is set in the
        # test definition.
        if platform.node() == "test-subvol-utils-inner":
            volume_tmp_dir = b"/"
        # This branch is used for "non-image" testing, ie: when the test is run
        # in the context of the host via a standard `python_unittest`.
        else:
            volume_tmp_dir = volume_dir() / "tmp"
            try:
                os.mkdir(volume_tmp_dir)
            except FileExistsError:
                pass

        with temp_dir(
            dir=volume_tmp_dir.decode(), prefix="delete_recursive"
        ) as td, _create_temp_subvol(
            td / "outer"
        ) as outer, _create_temp_subvol(
            td / "outer/inner1"
        ) as inner1, _create_temp_subvol(
            td / "outer/inner1/inner2"
        ) as inner2, _create_temp_subvol(
            td / "outer/inner3"
        ):

            inner2.set_readonly(True)
            inner1.set_readonly(True)
            outer.delete()
            self.assertEqual([], td.listdir())
