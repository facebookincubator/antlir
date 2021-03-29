#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import sys

from antlir.compiler.requires_provides import (
    RequireGroup,
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    ProvidesGroup,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes

from ..phases_provide import PhasesProvideItem, gen_subvolume_subtree_provides
from .common import (
    BaseItemTestCase,
    populate_temp_filesystem,
    temp_filesystem_provides,
)


class PhaseProvidesItemTestCase(BaseItemTestCase):
    def test_phases_provide(self):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            parent = temp_subvolumes.create("parent")
            # Permit _populate_temp_filesystem to make writes.
            parent.run_as_root(
                [
                    "chown",
                    "--no-dereference",
                    f"{os.geteuid()}:{os.getegid()}",
                    parent.path(),
                ]
            )
            populate_temp_filesystem(parent.path().decode())

            with self.assertRaises(subprocess.CalledProcessError):
                list(
                    gen_subvolume_subtree_provides(parent, Path("no_such/path"))
                )

            for create_meta in [False, True]:
                # Check that we properly handle ignoring a /.meta if it's
                # present
                if create_meta:
                    parent.run_as_root(["mkdir", parent.path(".meta")])
                self._check_item(
                    PhasesProvideItem(from_target="t", subvol=parent),
                    temp_filesystem_provides()
                    | {
                        ProvidesDirectory(path=Path("/")),
                        ProvidesDoNotAccess(path=Path("/.meta")),
                    },
                    set(),
                )

    def test_phases_provide_groups(self):
        with TempSubvolumes() as ts:
            sv = ts.create("test_phases_provide_groups")
            sv.run_as_root(["mkdir", "-p", sv.path("/etc")]).check_returncode()
            sv.run_as_root(
                ["tee", sv.path("/etc/group")],
                input=b"""root:x:0:
bin:x:1:
daemon:x:2:
sys:x:3:
adm:x:4:
""",
            ).check_returncode()

            self.assertEqual(
                set(PhasesProvideItem(from_target="t", subvol=sv).provides()),
                {
                    ProvidesDirectory(path=Path("/")),
                    ProvidesDoNotAccess(path=Path("/.meta")),
                    ProvidesDirectory(path=Path("/etc")),
                    ProvidesFile(path=Path("/etc/group")),
                    ProvidesGroup("root"),
                    ProvidesGroup("bin"),
                    ProvidesGroup("daemon"),
                    ProvidesGroup("sys"),
                    ProvidesGroup("adm"),
                },
            )
