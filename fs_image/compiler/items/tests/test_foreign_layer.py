#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import shlex
import subprocess
import sys
import unittest

from fs_image.common import load_location
from fs_image.find_built_subvol import find_built_subvol
from fs_image.tests.temp_subvolumes import TempSubvolumes

from ..common import PhaseOrder
from ..foreign_layer import ForeignLayerItem

from .common import DUMMY_LAYER_OPTS


def _touch_cmd(path: str):
    return ('/bin/sh', '-c', f'exec -a touch /bin/sh {shlex.quote(path)}')


class ForeignLayerItemTestCase(unittest.TestCase):

    def test_foreign_layer_item(self):
        parent_sv = find_built_subvol(
            load_location(__package__, 'foreign-layer-base')
        )
        # Works because we expect this to be world-executable, not just for root
        self.assertTrue(os.access(parent_sv.path('/bin/sh'), os.X_OK))
        with TempSubvolumes(sys.argv[0]) as temp_subvols:
            foreign_sv = temp_subvols.snapshot(parent_sv, 'foreign_layer')

            item = ForeignLayerItem(
                from_target='t', user='root', cmd=_touch_cmd('/HELLO_ALIEN'),
            )
            self.assertEqual(item.phase_order(), PhaseOrder.FOREIGN_LAYER)
            ForeignLayerItem.get_phase_builder(
                [item], DUMMY_LAYER_OPTS
            )(foreign_sv)

            alien_path = foreign_sv.path('/HELLO_ALIEN')
            self.assertTrue(os.path.isfile(alien_path))
            alien_stat = os.stat(alien_path)
            self.assertEqual((0, 0), (alien_stat.st_uid, alien_stat.st_gid))

            # Fail to write to `/meta`
            build_writes_to_meta = ForeignLayerItem.get_phase_builder([
                ForeignLayerItem(
                    from_target='t', user='root', cmd=_touch_cmd('/meta/ALIEN'),
                ),
            ], DUMMY_LAYER_OPTS)
            with self.assertRaises(subprocess.CalledProcessError):
                build_writes_to_meta(foreign_sv)
            self.assertTrue(os.path.isdir(foreign_sv.path('/meta')))
            self.assertFalse(os.path.exists(foreign_sv.path('/meta/ALIEN')))

            # `__fs_image__` is also protected
            foreign_sv.run_as_root(['mkdir', foreign_sv.path('/__fs_image__')])
            build_writes_to_fs_image = ForeignLayerItem.get_phase_builder([
                ForeignLayerItem(
                    from_target='t', user='root',
                    cmd=_touch_cmd('/__fs_image__/ALIEN'),
                ),
            ], DUMMY_LAYER_OPTS)
            with self.assertRaises(subprocess.CalledProcessError):
                build_writes_to_fs_image(foreign_sv)
            self.assertEqual([], foreign_sv.path('/__fs_image__').listdir())
