# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.config import antlir_dep
from antlir.rpm.find_snapshot import snapshot_install_dir

from antlir.rpm.replay.subvol_rpm_compare import (
    NEVRA,
    subvol_rpm_compare,
    subvol_rpm_compare_and_download,
    SubvolsToCompare,
)
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol
from antlir.tests.layer_resource import layer_resource_subvol


class SubvolRpmCompareTestImpl:
    def construct_subvols_to_compare(
        self, root: Subvol = None, leaf: Subvol = None, ba: Subvol = None
    ) -> SubvolsToCompare:
        root = root or layer_resource_subvol(__package__, "root_subvol")
        leaf = leaf or layer_resource_subvol(__package__, "leaf_subvol")
        ba = ba or layer_resource_subvol(__package__, "ba_subvol")

        return SubvolsToCompare(
            ba=ba,
            root=root,
            leaf=leaf,
            rpm_installer=self._YUM_DNF,
            rpm_repo_snapshot=snapshot_install_dir(
                antlir_dep("rpm:rpm-replay-repo-snapshot-for-tests")
            ),
        )

    def test_subvol_rpm_compare_identical_subvols(self):
        root_subvol = layer_resource_subvol(__package__, "root_subvol")

        subvols = self.construct_subvols_to_compare(root=root_subvol, leaf=root_subvol)
        rd = subvol_rpm_compare(subvols=subvols)

        # if root == leaf then no rpms should added/removed
        self.assertEqual(len(rd.added_in_order), 0)
        self.assertEqual(len(rd.removed), 0)

    def test_subvol_rpm_compare_added_order(self):
        subvols = self.construct_subvols_to_compare()
        rd = subvol_rpm_compare(subvols=subvols)
        rpms_added_names = [nevra.name for nevra in rd.added_in_order]
        rpms_with_deps = [
            "rpm-test-first",
            "rpm-test-second",
            "rpm-test-third",
            "rpm-test-fourth",
            "rpm-test-fifth",
        ]
        self.assertIn(
            rpms_added_names,
            [
                # Since `has-epoch` and `mice` has no deps or dependents,
                # it could go in any order
                [*rpms_with_deps, "rpm-test-mice", "rpm-test-has-epoch"],
                [*rpms_with_deps, "rpm-test-has-epoch", "rpm-test-mice"],
                ["rpm-test-mice", "rpm-test-has-epoch", *rpms_with_deps],
                ["rpm-test-has-epoch", "rpm-test-mice", *rpms_with_deps],
                ["rpm-test-has-epoch", *rpms_with_deps, "rpm-test-mice"],
                ["rpm-test-mice", *rpms_with_deps, "rpm-test-has-epoch"],
            ],
        )

        # mice should be upgraded to 0.2 version
        self.assertIn(
            NEVRA("rpm-test-mice", "0", "0.2", "a", "x86_64"), rd.added_in_order
        )

        self.assertEqual(
            {
                # mice upgrade causes 0.1 version to be removed
                NEVRA("rpm-test-mice", "0", "0.1", "a", "x86_64"),
                NEVRA("rpm-test-milk", "0", "2.71", "8", "x86_64"),
            },
            rd.removed,
        )

    def test_subvol_rpm_compare_and_download(self):
        subvols = self.construct_subvols_to_compare()
        with subvol_rpm_compare_and_download(subvols) as (
            rpm_diff,
            rpm_download_subvol,
        ):
            downloaded_rpms = {f"{rpm}" for rpm in rpm_download_subvol.path().listdir()}
            self.assertEqual(
                {
                    "rpm-test-has-epoch-0-0.x86_64.rpm",
                    "rpm-test-first-0-0.x86_64.rpm",
                    "rpm-test-second-0-0.x86_64.rpm",
                    "rpm-test-third-0-0.x86_64.rpm",
                    "rpm-test-fourth-0-0.x86_64.rpm",
                    "rpm-test-fifth-0-0.x86_64.rpm",
                    "rpm-test-mice-0.2-a.x86_64.rpm",
                },
                downloaded_rpms,
            )


class YumSubvolRpmCompareTestCase(SubvolRpmCompareTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.yum


class DnfSubvolRpmCompareTestCase(SubvolRpmCompareTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.dnf
