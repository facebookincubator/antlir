#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.rpm.find_snapshot import mangle_target, snapshot_install_dir


class TestCommon(unittest.TestCase):
    def test_mangle_target(self) -> None:
        self.assertEqual(
            "non-default-rep...pshot-for-tests__XFsi7Ukto4Zfl6vp4e0F",
            mangle_target("//antlir/rpm:non-default-repo-snapshot-for-tests"),
        )
        self.assertEqual(
            "repo-snapshot-for-tests__s1VpIcnIj1lvTFLdIa-8",
            mangle_target("//antlir/rpm:repo-snapshot-for-tests"),
        )

    def test_snapshot_install_dir(self) -> None:
        self.assertEqual(
            b"/__antlir__/rpm/repo-snapshot/chicken__DPPfV4lnzLJ-mvxxFHHM",
            snapshot_install_dir("//well/fed:chicken"),
        )
