#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest
import subprocess

from fs_image.fs_utils import Path, temp_dir
from fs_image.rpm.find_snapshot import snapshot_install_dir


class ImageUnittestTestRepoServer(unittest.TestCase):

    def test_install_rpm(self):
        # Check all available package managers.
        snapshot_bin = snapshot_install_dir(
            '//fs_image/rpm:repo-snapshot-for-tests'
        ) / 'bin'
        package_mgr_bins = snapshot_bin.listdir()
        self.assertNotEqual([], package_mgr_bins)
        # We may lose this assertion later, but for now check explicitly
        # that both binaries are tested.
        self.assertEqual({b'dnf', b'yum'}, set(package_mgr_bins))
        for prog in package_mgr_bins:
            with temp_dir() as td:
                os.mkdir(td / 'meta')
                subprocess.check_call([
                    snapshot_bin / prog,
                    f'--installroot={td}',
                    'install', '--assumeyes', 'rpm-test-carrot',
                ])
                # We don't need a full rendered subvol test, since the
                # contents of the filesystem is checked by other tests.
                # (e.g.  `test-yum-dnf-from-snapshot`, `test-image-layer`)
                with open(td / 'rpm_test/carrot.txt') as infile:
                    self.assertEqual('carrot 2 rc0\n', infile.read())
