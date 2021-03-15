#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
A simple wrapper around subvol.run_as_root() and non-booted run_nspawn()
"""
import os
import pwd
from typing import AnyStr, List

from antlir.fs_utils import Path, generate_work_dir
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol


class BuildAppliance:
    def __init__(self, subvol: Subvol, build_appliance: Subvol):
        self._subvol = subvol
        self._build_appliance = build_appliance
        self._work_dir = generate_work_dir()

    def path(self, rel_path: Path):
        return os.path.join(self._work_dir, rel_path)

    # Future work: we need somehow verify inside the BuildAppliance class that
    # any path passed to it has either 1) been converted to a BA relative path
    # or 2) can be converted to a BA relative path automagically.  This should
    # also then involve a strong assertion and fail hard if either case cannot
    # be met.
    def run(self, cmd: List[AnyStr], **kwargs):
        opts = new_nspawn_opts(
            cmd=cmd,
            layer=self._build_appliance,
            bindmount_rw=[(self._subvol.path(), self._work_dir)],
            user=pwd.getpwnam("root"),
            **kwargs,
        )
        run_nspawn(opts, PopenArgs())
