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
from typing import Callable

from antlir.compiler.items.group import GroupFile
from antlir.compiler.items.user import PasswdFile
from antlir.errors import UserError

from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol


def _read_passwd(subvol: Subvol) -> PasswdFile:
    try:
        with open(subvol.path("etc/passwd"), "r") as f:
            return PasswdFile(f.read())
    except FileNotFoundError:
        with open("/etc/passwd", "r") as f:
            return PasswdFile(f.read())


def _read_group(subvol: Subvol) -> GroupFile:
    try:
        with open(subvol.path("etc/group"), "r") as f:
            return GroupFile(f.read())
    except FileNotFoundError:
        with open("/etc/group", "r") as f:
            return GroupFile(f.read())


def _recursive_fs_op(path: Path, func: Callable[[Path], None]) -> None:
    func(path)

    if not os.path.isdir(path):
        return

    def _raise_onerror(e: Exception) -> None:
        raise e

    for root, dirs, files in os.walk(path, onerror=_raise_onerror):
        for d in dirs:
            func(Path(os.path.join(root, d)))

        for f in files:
            func(Path(os.path.join(root, f)))


# Future: this should validate that the user & group actually exist in the
# image's passwd/group databases (blocked on having those be first-class
# objects in the image build process).
def build_stat_options(
    item,
    subvol: Subvol,
    full_target_path: Path,
    *,
    do_not_set_mode=False,
    build_appliance=None,
):
    assert full_target_path.startswith(
        subvol.path()
    ), "{self}: A symlink to {full_target_path} would point outside the image"

    if do_not_set_mode:
        assert getattr(item, "mode", None) is None, item

    if build_appliance:
        passwd_file = _read_passwd(subvol)
        group_file = _read_group(subvol)

        try:
            uid = int(item.user)
        except ValueError:
            uid = passwd_file.uid(item.user)
            if uid is None:
                raise UserError(
                    f"user '{item.user}' to own '{full_target_path}' "
                    f"does not exist in {subvol.path()}"
                )
        try:
            gid = int(item.group)
        except ValueError:
            gid = group_file.gid(item.group)
            if gid is None:
                raise UserError(
                    f"group '{item.group}' to own '{full_target_path}' "
                    f"does not exist in {subvol.path()}"
                )

        if not do_not_set_mode:
            _recursive_fs_op(
                full_target_path, lambda path: os.chmod(path, item.mode)
            )

        _recursive_fs_op(
            full_target_path,
            lambda path: os.chown(path, uid, gid, follow_symlinks=False),
        )

    else:
        if not do_not_set_mode:
            # -R is not a problem since it cannot be the case that we are
            # creating a directory that already has something inside it.  On the
            # plus side, it helps with nested directory creation.
            subvol.run_as_root(
                [
                    "chmod",
                    "--recursive",
                    f"{item.mode:o}",
                    full_target_path,
                ]
            )
        subvol.run_as_root(
            [
                "chown",
                "--no-dereference",
                "--recursive",
                f"{item.user}:{item.group}",
                full_target_path,
            ],
        )
