#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest

from fs_image.rpm.rpm_metadata import RpmMetadata


class ToyRpmBuildUnittestTest(unittest.TestCase):
    def test_built_files(self):
        # This test doesn't need to be a "rendered subvol" test, because we
        # only really care about the files that `image_rpmbuild_layer`
        # outputs into `/rpmbuild/RPMS/`, everything else is not part of the
        # contract, and is disposable.

        # Files added as part of the rpmbuild_layer
        self.assertTrue(os.path.exists("/rpmbuild/SOURCES/toy_src_file"))
        self.assertTrue(os.path.exists("/rpmbuild/SPECS/specfile.spec"))

        # Built from rpmbuild
        self.assertEqual(["toy.rpm"], os.listdir("/rpmbuild/RPMS"))
        rpm_path = b"/rpmbuild/RPMS/toy.rpm"
        self.assertTrue(os.path.exists(rpm_path))

        a = RpmMetadata.from_file(rpm_path)
        self.assertEqual(a.epoch, 0)
        self.assertEqual(a.version, "1.0")
        self.assertEqual(a.release, "1")

        # Check files contained in rpm
        files = subprocess.check_output(["rpm", "-qlp", rpm_path], text=True)
        self.assertEqual(files, "/usr/bin/toy_src_file\n")

    def test_build_dep(self):
        output = subprocess.check_output(["rpm", "-qa", "tree"], text=True)
        self.assertIn("tree-", output)
