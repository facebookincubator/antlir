#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import subprocess

from .helpers import get_layer

from .rpm_feature_test import RpmFeatureTest

RPM_TEST_LAYER = get_layer(
    importlib.resources.contents(__package__), "test_layer_"
)


class RpmRemoveTest(RpmFeatureTest):
    def test_rpms_remove(self):
        rpms_installed = subprocess.check_output(["rpm", "-qa"], text=True)

        if "antlir-test" in RPM_TEST_LAYER:
            rpms = {"rpm-test-carrot", "rpm-test-cheese", "rpm-test-milk"}

            if RPM_TEST_LAYER == "rpms-remove-antlir-test":
                should_be_installed = set()

        else:
            rpms = {"chef", "clang", "cowsay", "netpbm"}

            if RPM_TEST_LAYER == "rpms-remove-centos8":
                should_be_installed = {"chef", "clang"}

        self.assertRpmsInstalled(rpms_installed, should_be_installed)
        self.assertRpmsNotInstalled(rpms_installed, rpms - should_be_installed)
