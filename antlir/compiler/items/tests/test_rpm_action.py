#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
from contextlib import contextmanager
from unittest import mock

from antlir.bzl_const import BZL_CONST
from antlir.fs_utils import Path, temp_dir
from antlir.rpm.rpm_metadata import RpmMetadata, compare_rpm_versions
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol, TempSubvolumes
from antlir.tests.flavor_helpers import get_rpm_installers_supported
from antlir.tests.subvol_helpers import check_common_rpm_render, pop_path

from ..common import PhaseOrder
from ..rpm_action import (
    RpmAction,
    RpmActionItem,
)
from .common import BaseItemTestCase, render_subvol
from .rpm_action_base import create_rpm_action_item, RpmActionItemTestBase


class InstallerIndependentRpmActionItemTest(BaseItemTestCase):
    "Tests not using self._YUM_DNF"

    def test_phase_orders(self):
        self.assertEqual(
            PhaseOrder.RPM_INSTALL,
            create_rpm_action_item(
                name="n", action=RpmAction.install
            ).phase_order(),
        )
        self.assertEqual(
            PhaseOrder.RPM_REMOVE,
            create_rpm_action_item(
                name="n", action=RpmAction.remove_if_exists
            ).phase_order(),
        )


class RpmActionItemTestImpl(RpmActionItemTestBase):
    "Subclasses run these tests with concrete values of `self._YUM_DNF`."

    def setUp(self):
        if self._YUM_DNF.value not in get_rpm_installers_supported():
            self.skipTest(
                f"'{self._YUM_DNF}' not in '{get_rpm_installers_supported()}'"
            )

    def test_rpm_action_item_build_appliance(self):
        self._check_rpm_action_item_build_appliance(
            Subvol("test-build-appliance", already_exists=True)
        )

    @contextmanager
    def _test_rpm_action_item_install_local_setup(self):
        parent_subvol = Subvol("test-with-no-rpm", already_exists=True)
        local_rpm_path = "/rpm-test-cheese-2-1.rpm"
        with TempSubvolumes() as temp_subvolumes:
            subvol = temp_subvolumes.snapshot(parent_subvol, "add_cheese")

            RpmActionItem.get_phase_builder(
                [
                    create_rpm_action_item(
                        source=local_rpm_path,
                        action=RpmAction.install,
                    )
                ],
                self._opts(),
            )(subvol)

            r = render_subvol(subvol)

            self.assertEqual(
                ["(Dir)", {"cheese2.txt": ["(File d45)"]}],
                pop_path(r, "rpm_test"),
            )

            yield r

    def _check_rpm_action_item_subvol(
        self, subvol, rpm_item: RpmActionItem, fs_render, *, opts=None
    ):
        RpmActionItem.get_phase_builder(
            [rpm_item], opts if opts else self._opts()
        )(subvol)
        subvol.run_as_root(
            [
                "rm",
                "-rf",
                subvol.path("dev"),
                subvol.path("etc"),
                subvol.path(".meta"),
                subvol.path("var"),
            ]
        )
        # pyre-fixme[16]: `RpmActionItemTestImpl` has no attribute `assertEqual`
        self.assertEqual(["(Dir)", fs_render], render_subvol(subvol))

    def test_version_lock(self):
        with TempSubvolumes() as temp_subvolumes, temp_dir() as td:
            with open(td / "vset", "w") as outfile:
                outfile.write("0\trpm-test-carrot\t1\tlockme\tx86_64")

            subvol = temp_subvolumes.create("rpm_ver_lock")
            subvol.run_as_root(["mkdir", subvol.path(".meta")])
            self._check_rpm_action_item_subvol(
                subvol,
                create_rpm_action_item(
                    name="rpm-test-carrot",
                    action=RpmAction.install,
                    flavor_to_version_set={
                        "antlir_test": (td / "vset").decode()
                    },
                ),
                {"rpm_test": ["(Dir)", {"carrot.txt": ["(File d16)"]}]},
            )

    def test_version_override(self):
        with TempSubvolumes() as temp_subvolumes, temp_dir() as td:
            with open(td / "vset", "w") as outfile:
                outfile.write("0\trpm-test-carrot\t1\tlockme\tx86_64")

            subvol = temp_subvolumes.create("rpm_ver_lock")
            subvol.run_as_root(["mkdir", subvol.path(".meta")])
            layer_opts = self._opts(version_set_override=td / "vset")
            self._check_rpm_action_item_subvol(
                subvol,
                create_rpm_action_item(
                    name="rpm-test-carrot",
                    action=RpmAction.install,
                ),
                {"rpm_test": ["(Dir)", {"carrot.txt": ["(File d16)"]}]},
                opts=layer_opts,
            )
            self.assertEquals(
                "carrot 1 lockme\n",
                subvol.path("/rpm_test/carrot.txt").read_text(),
            )

    def test_version_override_with_dependency(self):
        with TempSubvolumes() as temp_subvolumes, temp_dir() as td:
            with open(td / "vset", "w") as outfile:
                outfile.write("0\trpm-test-carrot\t1\tlockme\tx86_64")

            subvol = temp_subvolumes.create("rpm_ver_lock")
            subvol.run_as_root(["mkdir", subvol.path(".meta")])

            def _self_check():
                layer_opts = self._opts(version_set_override=td / "vset")
                self._check_rpm_action_item_subvol(
                    subvol,
                    create_rpm_action_item(
                        name="rpm-test-veggie",
                        action=RpmAction.install,
                    ),
                    {
                        "rpm_test": [
                            "(Dir)",
                            {
                                "carrot.txt": ["(File d16)"],
                                "veggie.txt": ["(File d13)"],
                            },
                        ]
                    },
                    opts=layer_opts,
                )

            if self._YUM_DNF == YumDnf.yum:
                with self.assertRaises(subprocess.CalledProcessError):
                    _self_check()
            else:
                _self_check()
                self.assertEquals(
                    "veggie 1 rc0\n",
                    subvol.path("/rpm_test/veggie.txt").read_text(),
                )

    def test_version_lock_and_override(self):
        with TempSubvolumes() as temp_subvolumes, temp_dir() as td:
            with open(td / "vset_version_lock", "w") as outfile:
                outfile.write("0\trpm-test-carrot\t2\trc0\tx86_64")
            with open(td / "vset_version_override", "w") as outfile:
                outfile.write("0\trpm-test-carrot\t1\tlockme\tx86_64")

            subvol = temp_subvolumes.create("rpm_ver_lock")
            subvol.run_as_root(["mkdir", subvol.path(".meta")])
            layer_opts = self._opts(
                version_set_override=td / "vset_version_override"
            )
            self._check_rpm_action_item_subvol(
                subvol,
                create_rpm_action_item(
                    name="rpm-test-carrot",
                    action=RpmAction.install,
                    flavor_to_version_set={
                        "antlir_test": (td / "vset_version_lock").decode()
                    },
                ),
                {"rpm_test": ["(Dir)", {"carrot.txt": ["(File d16)"]}]},
                opts=layer_opts,
            )
            self.assertEquals(
                "carrot 1 lockme\n",
                subvol.path("/rpm_test/carrot.txt").read_text(),
            )

    def test_rpm_action_item_auto_downgrade(self):
        parent_subvol = Subvol("test-with-one-local-rpm", already_exists=True)
        src_rpm = Path("/rpm-test-cheese-1-1.rpm")

        with TempSubvolumes() as temp_subvolumes:
            # ensure cheese2 is installed in the parent from rpm-test-cheese-2-1
            assert os.path.isfile(parent_subvol.path("/rpm_test/cheese2.txt"))
            # make sure the RPM we are installing is older in order to
            # trigger the downgrade
            src_data = RpmMetadata.from_file(src_rpm)
            subvol_data = RpmMetadata.from_subvol(
                parent_subvol,
                Subvol("test-build-appliance", already_exists=True),
                src_data.name,
            )
            assert compare_rpm_versions(src_data, subvol_data) < 0

            subvol = temp_subvolumes.snapshot(parent_subvol, "rpm_action")
            self._check_rpm_action_item_subvol(
                subvol,
                create_rpm_action_item(
                    source=src_rpm, action=RpmAction.install
                ),
                {"rpm_test": ["(Dir)", {"cheese1.txt": ["(File d42)"]}]},
            )

    def _check_cheese_removal(self, local_rpm_path: Path):
        parent_subvol = Subvol("test-with-one-local-rpm", already_exists=True)
        with TempSubvolumes() as temp_subvolumes:
            # ensure cheese2 is installed in the parent from rpm-test-cheese-2-1
            assert os.path.isfile(parent_subvol.path("/rpm_test/cheese2.txt"))
            subvol = temp_subvolumes.snapshot(parent_subvol, "remove_cheese")
            self._check_rpm_action_item_subvol(
                subvol,
                create_rpm_action_item(
                    source=local_rpm_path,
                    action=RpmAction.remove_if_exists,
                ),
                {},  # No more `rpm_test/cheese2.txt` here.
            )

    def test_rpm_action_item_remove_local(self):
        # We expect the removal to be based just on the name of the RPM
        # in the metadata, so removing cheese-2 should be fine via either:
        for ver in [1, 2]:
            self._check_cheese_removal(f"/rpm-test-cheese-{ver}-1.rpm")

    def test_rpm_action_conflict(self):
        # Test both install-install, install-remove, and install-downgrade
        # conflicts.
        for rpm_actions in (
            (("cat", RpmAction.install), ("cat", RpmAction.install)),
            (("dog", RpmAction.remove_if_exists), ("dog", RpmAction.install)),
        ):
            with self.assertRaisesRegex(RuntimeError, "RPM action conflict "):
                # Note that we don't need to run the builder to hit the error
                RpmActionItem.get_phase_builder(
                    [
                        create_rpm_action_item(name=r, action=a)
                        for r, a in rpm_actions
                    ],
                    self._opts(),
                )

        with self.assertRaisesRegex(RuntimeError, "RPM action conflict "):
            # An extra test case for local RPM name conflicts (filenames are
            # different but RPM names are the same)
            RpmActionItem.get_phase_builder(
                [
                    create_rpm_action_item(
                        source="/rpm-test-cheese-2-1.rpm",
                        action=RpmAction.install,
                    ),
                    create_rpm_action_item(
                        source="/rpm-test-cheese-1-1.rpm",
                        action=RpmAction.remove_if_exists,
                    ),
                ],
                self._opts(),
            )

    def test_rpm_action_reinstall_same_exact_version(self):
        # installing the same exact version as an already installed package is
        # an explicit no-op
        parent_subvol = Subvol("test-with-one-local-rpm", already_exists=True)
        local_rpm_path = "/rpm-test-cheese-2-1.rpm"
        with TempSubvolumes() as temp_subvolumes:
            # ensure cheese2 is installed in the parent from rpm-test-cheese-2-1
            assert os.path.isfile(parent_subvol.path("/rpm_test/cheese2.txt"))
            subvol = temp_subvolumes.snapshot(parent_subvol, "remove_cheese")
            RpmActionItem.get_phase_builder(
                [
                    create_rpm_action_item(
                        source=local_rpm_path,
                        action=RpmAction.install,
                    )
                ],
                self._opts(),
            )(subvol)
            # cheese2 file is still there
            assert os.path.isfile(parent_subvol.path("/rpm_test/cheese2.txt"))

    def test_rpm_action_skip_wrong_flavor(self):
        with TempSubvolumes() as temp_subvolumes:
            src_rpm = Path("/rpm-test-cheese-1-1.rpm")
            subvol = temp_subvolumes.create("subvol")
            self._check_rpm_action_item_subvol(
                subvol,
                create_rpm_action_item(
                    source=src_rpm,
                    action=RpmAction.install,
                    flavor_to_version_set={
                        "wrong": BZL_CONST.version_set_allow_all_versions
                    },
                ),
                {},
            )


@mock.patch(
    "antlir.subvol_utils._tmp_volume_dir", mock.Mock(return_value=Path("/"))
)
class YumRpmActionItemTestCase(RpmActionItemTestImpl, BaseItemTestCase):
    _YUM_DNF = YumDnf.yum

    def test_rpm_action_item_install_local_yum(self):
        with self._test_rpm_action_item_install_local_setup() as r:
            check_common_rpm_render(self, r, "yum")


@mock.patch(
    "antlir.subvol_utils._tmp_volume_dir", mock.Mock(return_value=Path("/"))
)
class DnfRpmActionItemTestCase(RpmActionItemTestImpl, BaseItemTestCase):
    _YUM_DNF = YumDnf.dnf

    def test_rpm_action_item_install_local_dnf(self):
        with self._test_rpm_action_item_install_local_setup() as r:
            pop_path(r, "var/lib/yum", None)
            pop_path(r, "var/log/yum.log", None)
            check_common_rpm_render(self, r, "dnf")
