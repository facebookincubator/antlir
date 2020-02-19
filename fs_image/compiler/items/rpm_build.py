#!/usr/bin/env python3
import pwd
import sys

from typing import Iterable

from fs_image.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from fs_image.nspawn_in_subvol.non_booted import run_non_booted_nspawn
from subvol_utils import Subvol

from .common import ImageItem, LayerOpts, PhaseOrder


class RpmBuildItem(metaclass=ImageItem):
    fields = ['rpmbuild_dir']

    def phase_order(self):
        return PhaseOrder.RPM_BUILD

    @classmethod
    def get_phase_builder(
        cls, items: Iterable['RpmBuildItem'], layer_opts: LayerOpts,
    ):
        item, = items
        assert isinstance(item, RpmBuildItem), item

        def builder(subvol: Subvol):
            # For rpmbuild:
            #   - define _topdir to move where the RPM gets built
            #   - use -bb so it only builds from the specfile
            #   - define _rpmfilename to strip version from the result files
            build_cmd = (
                f"rpmbuild --define '_topdir {item.rpmbuild_dir}' "
                    "--define '_rpmfilename %%{NAME}.rpm' "
                    f"-bb {item.rpmbuild_dir}/SPECS/specfile.spec"
            )
            run_non_booted_nspawn(new_nspawn_opts(
                cmd=['sh', '-c', f'{build_cmd}'],
                layer=subvol,
                user=pwd.getpwnam('root'),
                snapshot=False,
            ), PopenArgs())

        return builder
