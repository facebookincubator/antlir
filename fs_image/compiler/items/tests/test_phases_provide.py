#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import sys

from compiler.requires_provides import ProvidesDirectory, ProvidesDoNotAccess
from tests.temp_subvolumes import TempSubvolumes

from ..phases_provide import PhasesProvideItem

from .common import (
    BaseItemTestCase, populate_temp_filesystem, temp_filesystem_provides,
)


class PhaseProvidesItemTestCase(BaseItemTestCase):

    def test_phases_provide(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            parent = temp_subvolumes.create('parent')
            # Permit _populate_temp_filesystem to make writes.
            parent.run_as_root([
                'chown', '--no-dereference', f'{os.geteuid()}:{os.getegid()}',
                parent.path(),
            ])
            populate_temp_filesystem(parent.path().decode())
            for create_meta in [False, True]:
                # Check that we properly handle ignoring a /meta if it's present
                if create_meta:
                    parent.run_as_root(['mkdir', parent.path('meta')])
                self._check_item(
                    PhasesProvideItem(from_target='t', subvol=parent),
                    temp_filesystem_provides() | {
                        ProvidesDirectory(path='/'),
                        ProvidesDoNotAccess(path='/meta'),
                    },
                    set(),
                )
