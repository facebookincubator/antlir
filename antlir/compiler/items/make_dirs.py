#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import pwd
from dataclasses import dataclass

from antlir.compiler.requires_provides import (
    ProvidesDirectory,
    require_directory,
)
from antlir.fs_utils import Path, generate_work_dir
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.non_booted import run_non_booted_nspawn
from antlir.subvol_utils import Subvol

from .common import ImageItem, LayerOpts, coerce_path_field_normal_relative
from .stat_options import Mode, build_stat_options, customize_stat_options


@dataclass(init=False, frozen=True)
class MakeDirsItem(ImageItem):
    into_dir: str
    path_to_make: str

    # Stat option fields
    mode: Mode
    user_group: str

    @classmethod
    def customize_fields(cls, kwargs):
        super().customize_fields(kwargs)
        coerce_path_field_normal_relative(kwargs, "into_dir")
        coerce_path_field_normal_relative(kwargs, "path_to_make")
        # Unlike files, leave directories as writable by the owner by
        # default, since it's reasonable for files to be added at runtime.
        customize_stat_options(kwargs, default_mode=0o755)

    def provides(self):
        inner_dir = os.path.join(self.into_dir, self.path_to_make)
        while inner_dir != self.into_dir:
            yield ProvidesDirectory(path=inner_dir)
            inner_dir = os.path.dirname(inner_dir)

    def requires(self):
        yield require_directory(self.into_dir)

    def build(self, subvol: Subvol, layer_opts: LayerOpts):
        if layer_opts.build_appliance:
            work_dir = generate_work_dir()
            full_path = Path(work_dir) / self.into_dir / self.path_to_make
            opts = new_nspawn_opts(
                cmd=["mkdir", "-p", full_path],
                layer=layer_opts.build_appliance,
                bindmount_rw=[(subvol.path(), work_dir)],
                user=pwd.getpwnam("root"),
            )
            run_non_booted_nspawn(opts, PopenArgs())
        else:
            inner_dir = subvol.path(
                os.path.join(self.into_dir, self.path_to_make)
            )
            subvol.run_as_root(["mkdir", "-p", inner_dir])
        outer_dir = self.path_to_make.split("/", 1)[0]
        build_stat_options(
            self,
            subvol,
            subvol.path(os.path.join(self.into_dir, outer_dir)),
            build_appliance=layer_opts.build_appliance,
        )
