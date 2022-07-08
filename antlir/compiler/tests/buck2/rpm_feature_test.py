#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest


class RpmFeatureTest(unittest.TestCase):
    def assertRpmsInstalled(self, rpms_installed, rpm_list):
        for rpm in rpm_list:
            self.assertIn(rpm, rpms_installed)

    def assertRpmsNotInstalled(self, rpms_installed, rpm_list):
        for rpm in rpm_list:
            self.assertNotIn(rpm, rpms_installed)
