#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import sys

from contextlib import contextmanager

from fs_image.btrfs_diff.tests.render_subvols import (
    check_common_rpm_render, pop_path
)
from fs_image.fs_utils import Path
from fs_image.rpm.rpm_metadata import RpmMetadata, compare_rpm_versions
from fs_image.rpm.yum_dnf_conf import YumDnf

from fs_image.tests.layer_resource import layer_resource_subvol
from fs_image.tests.temp_subvolumes import TempSubvolumes

from ..common import PhaseOrder
from ..rpm_action import RpmAction, RpmActionItem

from .common import BaseItemTestCase, get_dummy_layer_opts_ba, render_subvol
from .rpm_action_base import RpmActionItemTestBase

DUMMY_LAYER_OPTS_BA = get_dummy_layer_opts_ba()


class InstallerIndependentRpmActionItemTest(BaseItemTestCase):
    'Tests not using self._YUM_DNF'

    def test_phase_orders(self):
        self.assertEqual(PhaseOrder.RPM_INSTALL, RpmActionItem(
            from_target='t', name='n', action=RpmAction.install,
        ).phase_order())
        self.assertEqual(PhaseOrder.RPM_REMOVE, RpmActionItem(
            from_target='t', name='n', action=RpmAction.remove_if_exists,
        ).phase_order())


class RpmActionItemTestImpl(RpmActionItemTestBase):
    'Subclasses run these tests with concrete values of `self._YUM_DNF`.'

    def test_rpm_action_item_build_appliance(self):
        self._check_rpm_action_item_build_appliance(layer_resource_subvol(
            __package__, 'host-test-build-appliance',
        ))

    def _opts(self):
        return DUMMY_LAYER_OPTS_BA._replace(
            rpm_installer=self._YUM_DNF,
        )

    @contextmanager
    def _test_rpm_action_item_install_local_setup(self):
        parent_subvol = layer_resource_subvol(__package__, 'test-with-no-rpm')
        local_rpm_path = Path(__file__).dirname() / 'rpm-test-cheese-2-1.rpm'
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.snapshot(parent_subvol, 'add_cheese')

            RpmActionItem.get_phase_builder(
                [RpmActionItem(
                    from_target='t',
                    source=local_rpm_path,
                    action=RpmAction.install,
                )],
                self._opts(),
            )(subvol)

            r = render_subvol(subvol)

            self.assertEqual(['(Dir)', {
                'cheese2.txt': ['(File d45)'],
                }], pop_path(r, 'rpm_test'))

            yield r

    def test_rpm_action_item_auto_downgrade(self):
        parent_subvol = layer_resource_subvol(
            __package__, 'test-with-one-local-rpm',
        )
        src_rpm = Path(__file__).dirname() / "rpm-test-cheese-1-1.rpm"

        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            # ensure cheese2 is installed in the parent from rpm-test-cheese-2-1
            assert os.path.isfile(
                parent_subvol.path('/rpm_test/cheese2.txt')
            )
            # make sure the RPM we are installing is older in order to
            # trigger the downgrade
            src_data = RpmMetadata.from_file(src_rpm)
            subvol_data = RpmMetadata.from_subvol(parent_subvol, src_data.name)
            assert compare_rpm_versions(src_data, subvol_data) < 0

            subvol = temp_subvolumes.snapshot(parent_subvol, 'rpm_action')
            RpmActionItem.get_phase_builder(
                [RpmActionItem(
                    from_target='t',
                    source=src_rpm,
                    action=RpmAction.install,
                )],
                self._opts(),
            )(subvol)
            subvol.run_as_root([
                'rm', '-rf',
                subvol.path('dev'),
                subvol.path('etc'),
                subvol.path('meta'),
                subvol.path('var'),
            ])
            self.assertEqual(['(Dir)', {
                'rpm_test': ['(Dir)', {
                    'cheese1.txt': ['(File d42)'],
                }],
            }], render_subvol(subvol))

    def _check_cheese_removal(self, local_rpm_path: Path):
        parent_subvol = layer_resource_subvol(
            __package__, 'test-with-one-local-rpm',
        )
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            # ensure cheese2 is installed in the parent from rpm-test-cheese-2-1
            assert os.path.isfile(
                parent_subvol.path('/rpm_test/cheese2.txt')
            )
            subvol = temp_subvolumes.snapshot(parent_subvol, 'remove_cheese')
            RpmActionItem.get_phase_builder(
                [RpmActionItem(
                    from_target='t',
                    source=local_rpm_path,
                    action=RpmAction.remove_if_exists,
                )],
                self._opts(),
            )(subvol)
            subvol.run_as_root([
                'rm', '-rf',
                subvol.path('dev'),
                subvol.path('etc'),
                subvol.path('meta'),
                subvol.path('var'),
            ])
            self.assertEqual(['(Dir)', {
                # No more `rpm_test/cheese2.txt` here.
            }], render_subvol(subvol))

    def test_rpm_action_item_remove_local(self):
        # We expect the removal to be based just on the name of the RPM
        # in the metadata, so removing cheese-2 should be fine via either:
        for ver in [1, 2]:
            self._check_cheese_removal(
                Path(__file__).dirname() / f'rpm-test-cheese-{ver}-1.rpm',
            )

    def test_rpm_action_conflict(self):
        # Test both install-install, install-remove, and install-downgrade
        # conflicts.
        for rpm_actions in (
            (('cat', RpmAction.install), ('cat', RpmAction.install)),
            (
                ('dog', RpmAction.remove_if_exists),
                ('dog', RpmAction.install),
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, 'RPM action conflict '):
                # Note that we don't need to run the builder to hit the error
                RpmActionItem.get_phase_builder(
                    [
                        RpmActionItem(from_target='t', name=r, action=a)
                            for r, a in rpm_actions
                    ],
                    self._opts(),
                )

        with self.assertRaisesRegex(RuntimeError, 'RPM action conflict '):
            # An extra test case for local RPM name conflicts (filenames are
            # different but RPM names are the same)
            RpmActionItem.get_phase_builder(
                [
                    RpmActionItem(
                        from_target='t',
                        source=Path(__file__).dirname() /
                            "rpm-test-cheese-2-1.rpm",
                        action=RpmAction.install,
                    ),
                    RpmActionItem(
                        from_target='t',
                        source=Path(__file__).dirname() /
                            "rpm-test-cheese-1-1.rpm",
                        action=RpmAction.remove_if_exists,
                    ),
                ],
                self._opts(),
            )


class YumRpmActionItemTestCase(RpmActionItemTestImpl, BaseItemTestCase):
    _YUM_DNF = YumDnf.yum

    def test_rpm_action_item_install_local_yum(self):
        with self._test_rpm_action_item_install_local_setup() as r:
            check_common_rpm_render(self, r, 'yum')


class DnfRpmActionItemTestCase(RpmActionItemTestImpl, BaseItemTestCase):
    _YUM_DNF = YumDnf.dnf

    def test_rpm_action_item_install_local_dnf(self):
        with self._test_rpm_action_item_install_local_setup() as r:
            pop_path(r, 'var/cache/yum')
            pop_path(r, 'var/lib/yum')
            pop_path(r, 'var/log/yum.log')
            check_common_rpm_render(self, r, 'dnf')
