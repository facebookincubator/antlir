#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import pwd
import sys

from dataclasses import dataclass
from typing import Iterable

from fs_image.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from fs_image.nspawn_in_subvol.non_booted import run_non_booted_nspawn
from subvol_utils import Subvol

from .common import ImageItem, LayerOpts, PhaseOrder


@dataclass(init=False, frozen=True)
class RpmBuildItem(ImageItem):
    rpmbuild_dir: str

    def phase_order(self):
        return PhaseOrder.RPM_BUILD

    @classmethod
    def get_phase_builder(
        cls, items: Iterable['RpmBuildItem'], layer_opts: LayerOpts,
    ):
        item, = items
        assert isinstance(item, RpmBuildItem), item

        def builder(subvol: Subvol):
            run_non_booted_nspawn(new_nspawn_opts(
                cmd=[
                    'rpmbuild',
                    # Change the destination for the built RPMs
                    f'--define=_topdir {item.rpmbuild_dir}',
                    # Don't include the version in the resulting RPM filenames
                    '--define=_rpmfilename %%{NAME}.rpm',
                    '-bb',  # Only build the binary packages (no SRPMs)
                    f'{item.rpmbuild_dir}/SPECS/specfile.spec',
                ],
                layer=subvol,
                user=pwd.getpwnam('root'),
                snapshot=False,
            ), PopenArgs())

        return builder
