#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import sys
import unittest

from fs_image.find_built_subvol import find_built_subvol
from fs_image.common import load_location

from fs_image.tests.temp_subvolumes import TempSubvolumes

from ..common import PhaseOrder
from ..rpm_build import RpmBuildItem

from .common import DUMMY_LAYER_OPTS


class RpmBuildItemTestCase(unittest.TestCase):

    def test_rpm_build_item(self):
        parent_subvol = find_built_subvol(load_location(
            'fs_image.compiler.items', 'toy-rpmbuild-setup',
        ))
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            assert os.path.isfile(
                parent_subvol.path('/rpmbuild/SOURCES/toy_src_file')
            )
            assert os.path.isfile(
                parent_subvol.path('/rpmbuild/SPECS/specfile.spec')
            )

            subvol = temp_subvolumes.snapshot(parent_subvol, 'rpm_build')
            item = RpmBuildItem(from_target='t', rpmbuild_dir='/rpmbuild')
            RpmBuildItem.get_phase_builder([item], DUMMY_LAYER_OPTS)(subvol)

            self.assertEqual(item.phase_order(), PhaseOrder.RPM_BUILD)
            assert os.path.isfile(
                subvol.path('/rpmbuild/RPMS/toy.rpm')
            )
