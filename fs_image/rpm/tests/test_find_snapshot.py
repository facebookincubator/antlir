#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from ..find_snapshot import mangle_target, snapshot_install_dir


class TestCommon(unittest.TestCase):
    def test_mangle_target(self):
        self.assertEqual(
            "non-default-rep...pshot-for-tests__3012b15a",
            mangle_target("//fs_image/rpm:non-default-repo-snapshot-for-tests"),
        )
        self.assertEqual(
            "repo-snapshot-for-tests__bd44ee8c",
            mangle_target("//fs_image/rpm:repo-snapshot-for-tests"),
        )

    def test_snapshot_install_dir(self):
        self.assertEqual(
            b"/__fs_image__/rpm/repo-snapshot/chicken__a08636a6",
            snapshot_install_dir("//well/fed:chicken"),
        )
