#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Helpers for setting `stat (2)` options on files, directories, etc, which
we are creating inside the image.
"""

import os
import pwd

from antlir.fs_utils import generate_work_dir
from antlir.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol


# Future: this should validate that the user & group actually exist in the
# image's passwd/group databases (blocked on having those be first-class
# objects in the image build process).
def build_stat_options(
    item,
    subvol: Subvol,
    full_target_path: str,
    *,
    do_not_set_mode=False,
    build_appliance=None,
):
    assert full_target_path.startswith(
        # pyre-fixme[6]: Expected `Union[str, typing.Tuple[str, ...]]` for 1st
        #  param but got `Path`.
        subvol.path()
    ), "{self}: A symlink to {full_target_path} would point outside the image"

    if build_appliance:
        work_dir = generate_work_dir()

        # Fall back to using the host passwd if `subvol` doesn't have the DBs.
        etc_passwd = subvol.path("/etc/passwd")
        etc_group = subvol.path("/etc/group")
        if not (etc_passwd.exists() and etc_group.exists()):
            etc_passwd = "/etc/passwd"
            etc_group = "/etc/group"

        def run(cmd, **kwargs):
            run_nspawn(
                new_nspawn_opts(
                    cmd=cmd,
                    layer=build_appliance,
                    bindmount_rw=[(subvol.path(), work_dir)],
                    bindmount_ro=[
                        (etc_passwd, "/etc/passwd"),
                        (etc_group, "/etc/group"),
                    ],
                    user=pwd.getpwnam("root"),
                    **kwargs,
                ),
                PopenArgs(),
            )

        target_path_for_run = work_dir / os.path.relpath(
            full_target_path,
            # pyre-fixme[6]: For 2nd param expected `Union[None, PathLike[str],
            #  str]` but got `Path`.
            subvol.path(),
        )
    else:
        run = subvol.run_as_root
        target_path_for_run = full_target_path

    # `chmod` lacks a --no-dereference flag to protect us from following
    # `full_target_path` if it's a symlink.  As far as I know, this should
    # never occur, so just let the exception fly.
    run(["test", "!", "-L", target_path_for_run])

    if do_not_set_mode:
        assert getattr(item, "mode", None) is None, item
    else:
        # -R is not a problem since it cannot be the case that we are
        # creating a directory that already has something inside it.  On the
        # plus side, it helps with nested directory creation.
        run(
            [
                "chmod",
                "--recursive",
                f"{item.mode:o}",
                target_path_for_run,
            ]
        )

    run(
        [
            "chown",
            "--no-dereference",
            "--recursive",
            f"{item.user}:{item.group}",
            target_path_for_run,
        ],
    )
