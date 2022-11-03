#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import List

from antlir.compiler.items.apt_action import AptActionItems
from antlir.compiler.items.common import PhaseOrder

from antlir.compiler.items.tests.common import BaseItemTestCase
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol

_DPKG_STATUS_FILE = "/var/lib/dpkg/status"


class AptActionItemsTestCase(BaseItemTestCase):
    def test_phase_order(self):
        self.assertEqual(
            PhaseOrder.APT_INSTALL,
            AptActionItems(package_names=["test"], action="install").phase_order(),
        )
        self.assertEqual(
            PhaseOrder.APT_REMOVE,
            AptActionItems(
                package_names=["test"], action="remove_if_exists"
            ).phase_order(),
        )

    def _test_packages(
        self,
        status_file_path: Path,
        assert_present_packages: List[str],
        assert_absent_packages: List[str],
    ):
        with open(status_file_path) as fd:
            status = fd.read().split("\n")
        for package in assert_present_packages:
            self.assertIn(f"Package: {package}", status)
        for package in assert_absent_packages:
            self.assertNotIn(f"Package: {package}", status)

    def test_base_layer(self):
        layer = Subvol("base_layer", already_exists=True)
        self._test_packages(
            status_file_path=layer.path(_DPKG_STATUS_FILE),
            assert_present_packages=[],
            assert_absent_packages=["zsh", "cowsay"],
        )

    def test_installed_apt_from_layer(self):
        layer = Subvol("packages_installed_layer", already_exists=True)
        self._test_packages(
            status_file_path=layer.path(_DPKG_STATUS_FILE),
            assert_present_packages=["zsh", "cowsay"],
            assert_absent_packages=[],
        )

    def test_removed_apt_from_layer(self):
        layer = Subvol("packages_removed_layer", already_exists=True)
        self._test_packages(
            status_file_path=layer.path(_DPKG_STATUS_FILE),
            assert_present_packages=["cowsay"],
            assert_absent_packages=["zsh"],
        )
