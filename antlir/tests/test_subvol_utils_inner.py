# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import platform
import unittest

from ..fs_utils import temp_dir
from ..subvol_utils import (
    Subvol,
    volume_dir,
)


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
        ) as td:
            try:
                outer = Subvol(td / "outer")
                outer.create()
                inner1 = Subvol(td / "outer/inner1")
                inner1.create()
                inner2 = Subvol(td / "outer/inner1/inner2")
                inner2.create()
                inner3 = Subvol(td / "outer/inner3")
                inner3.create()

                outer.delete()
                self.assertEqual([], td.listdir())
            except BaseException:  # Clean up even on Ctrl-C
                try:
                    inner2.delete()
                finally:
                    try:
                        inner1.delete()
                    finally:
                        try:
                            inner3.delete()
                        finally:
                            outer.delete()
                raise
