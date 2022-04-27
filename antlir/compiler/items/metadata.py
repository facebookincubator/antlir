# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import getpass
import os
from dataclasses import dataclass
from typing import Iterable

from antlir.config import repo_config
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol

from .common import ImageItem, LayerOpts, PhaseOrder

_METADATA_PATH = Path("/.meta/build")


@dataclass(init=False, frozen=True)
class LayerInfoItem(ImageItem):
    def phase_order(self):
        return PhaseOrder.LAYER_INFO

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["LayerInfoItem"], layer_opts: LayerOpts
    ):
        def builder(subvol: Subvol):
            # Can do this only if I am 'root'
            if getpass.getuser() != "root":  # pragma: no cover
                return

            if not os.path.isdir(
                subvol.path(_METADATA_PATH)
            ):  # pragma: no cover
                os.makedirs(subvol.path(_METADATA_PATH), mode=0o755)

            with open(subvol.path(_METADATA_PATH / "target"), "w") as f:
                f.write(f"{layer_opts.layer_target}\n")

            with open(subvol.path(_METADATA_PATH / "revision"), "w") as f:
                f.write(f"{repo_config().vcs_revision}\n")

        return builder
