# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import errno
import os
import platform
import unittest

from antlir import btrfsutil

from antlir.fs_utils import temp_dir
from antlir.subvol_utils import Subvol, volume_dir


class InnerSubvolTestCase(unittest.TestCase):
    def test_delete_inner_subvols(self) -> None:
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
            with Subvol(td / "outer").create().delete_on_exit() as outer:
                inner1 = Subvol(td / "outer/inner1").create()
                inner2 = Subvol(td / "outer/inner1/inner2").create()
                Subvol(td / "outer/inner3").create()
                inner2.set_readonly(True)
                inner1.set_readonly(True)
                self.assertTrue(btrfsutil.is_subvolume(outer.path()))
                # pass useful to show if any exceptions are happening inside the
                # context manager
                pass
            # show that the subvol was deleted by the delete_on_exit context,
            # not just temp_dir
            self.assertFalse(os.path.exists(outer.path()))
            pass
        with self.assertRaises(btrfsutil.BtrfsUtilError) as e:
            self.assertFalse(btrfsutil.is_subvolume(outer.path()))
        self.assertEqual(e.exception.errno, errno.ENOENT)
