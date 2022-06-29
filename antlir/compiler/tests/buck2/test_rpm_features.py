#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import subprocess
import unittest

TARGET_RESOURCE_PREFIX = "test_layer_"

RPM_TEST_LAYER = None
for target in importlib.resources.contents(__package__):
    if target.startswith(TARGET_RESOURCE_PREFIX):
        RPM_TEST_LAYER = target[len(TARGET_RESOURCE_PREFIX) :]
        break
if not RPM_TEST_LAYER:
    raise RuntimeError("`RPM_TEST_LAYER` undefined")


class RpmFeatureTest(unittest.TestCase):
    def assertRpmsInstalled(self, rpms_installed, rpm_list):
        for rpm in rpm_list:
            self.assertIn(rpm, rpms_installed)

    def assertRpmsNotInstalled(self, rpms_installed, rpm_list):
        for rpm in rpm_list:
            self.assertNotIn(rpm, rpms_installed)

    def test_rpms_install(self):
        rpms_installed = subprocess.check_output(["rpm", "-qa"], text=True)

        rpms = {"chef", "clang", "cowsay", "netpbm"}

        if RPM_TEST_LAYER == "rpms-install-centos7":
            should_be_installed = {"chef"}
        elif RPM_TEST_LAYER == "rpms-install-centos8":
            should_be_installed = {"chef"}
        elif RPM_TEST_LAYER == "rpms-install-centos8-untested":
            should_be_installed = {"chef"}
        elif RPM_TEST_LAYER == "rpms-install-centos9":
            should_be_installed = {"chef"}
        elif RPM_TEST_LAYER == "rpms-install-centos9-untested":
            should_be_installed = {"chef"}
        elif RPM_TEST_LAYER == "rpms-install-centos7-child":
            should_be_installed = {"chef", "clang", "cowsay"}
        elif RPM_TEST_LAYER == "rpms-install-centos8-child":
            should_be_installed = {"chef", "clang", "netpbm"}

        self.assertRpmsInstalled(rpms_installed, should_be_installed)
        self.assertRpmsNotInstalled(rpms_installed, rpms - should_be_installed)
