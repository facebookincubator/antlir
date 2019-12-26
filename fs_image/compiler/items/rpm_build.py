#!/usr/bin/env python3
import sys

from typing import Iterable

from nspawn_in_subvol import nspawn_in_subvol, \
    parse_opts as nspawn_in_subvol_parse_opts
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

            opts = nspawn_in_subvol_parse_opts([
                '--layer', 'UNUSED',
                '--user', 'root',
                '--no-snapshot',
                '--',
                'sh', '-c', f'{build_cmd}',
            ])
            nspawn_in_subvol(subvol, opts, stdout=sys.stderr)

        return builder
