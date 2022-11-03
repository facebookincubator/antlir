#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import sys

from antlir.compiler.items.group import GROUP_FILE_PATH
from antlir.compiler.items.phases_provide import (
    gen_subvolume_subtree_provides,
    PhasesProvideItem,
)
from antlir.compiler.items.tests.common import (
    BaseItemTestCase,
    populate_temp_filesystem,
    temp_filesystem_provides,
)
from antlir.compiler.items.user import PASSWD_FILE_PATH

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    ProvidesDoNotAccess,
    ProvidesFile,
    ProvidesGroup,
    ProvidesUser,
)
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes


class PhaseProvidesItemTestCase(BaseItemTestCase):
    def test_phases_provide(self) -> None:
        with TempSubvolumes(Path(sys.argv[0])) as temp_subvolumes:
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
                list(gen_subvolume_subtree_provides(parent, Path("no_such/path")))

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
                        ProvidesUser("root"),
                        ProvidesGroup("root"),
                    },
                    set(),
                )

    def test_phases_provide_groups(self) -> None:
        with TempSubvolumes() as ts:
            sv = ts.create("test_phases_provide_groups")
            sv.run_as_root(["mkdir", "-p", sv.path("/etc")]).check_returncode()
            sv.overwrite_path_as_root(
                GROUP_FILE_PATH,
                """root:x:0:
bin:x:1:
daemon:x:2:
sys:x:3:
adm:x:4:
""",
            )

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
                    ProvidesUser("root"),
                },
            )

    def test_phases_provide_users(self) -> None:
        with TempSubvolumes() as ts:
            sv = ts.create("test_phases_provide_users")
            sv.run_as_root(["mkdir", "-p", sv.path("/etc")]).check_returncode()
            sv.overwrite_path_as_root(
                PASSWD_FILE_PATH,
                """root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
daemon:x:2:2:daemon:/sbin:/sbin/nologin
adm:x:3:4:adm:/var/adm:/sbin/nologin
lp:x:4:7:lp:/var/spool/lpd:/sbin/nologin
""",
            )

            self.assertEqual(
                set(PhasesProvideItem(from_target="t", subvol=sv).provides()),
                {
                    ProvidesDirectory(path=Path("/")),
                    ProvidesDoNotAccess(path=Path("/.meta")),
                    ProvidesDirectory(path=Path("/etc")),
                    ProvidesFile(path=Path("/etc/passwd")),
                    ProvidesUser("root"),
                    ProvidesUser("bin"),
                    ProvidesUser("daemon"),
                    ProvidesUser("adm"),
                    ProvidesUser("lp"),
                    ProvidesGroup("root"),
                },
            )
