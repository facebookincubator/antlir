# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from contextlib import contextmanager

from antlir.fs_utils import ANTLIR_DIR, CP_CLONE_CMD
from antlir.nspawn_in_subvol.args import _NspawnOpts
from antlir.nspawn_in_subvol.plugin_hooks import (
    _NspawnSetup,
    _NspawnSetupCtxMgr,
    _SetupSubvolCtxMgr,
)

from antlir.nspawn_in_subvol.plugins import NspawnPlugin


class AttachAntlirDir(NspawnPlugin):
    @contextmanager
    def wrap_setup_subvol(
        self,
        setup_subvol_ctx: _SetupSubvolCtxMgr,
        opts: _NspawnOpts,
    ) -> _NspawnSetup:
        with setup_subvol_ctx(opts) as subvol:
            subvol_antlir_dir = subvol.path(ANTLIR_DIR)
            build_appliance_antlir_dir = (
                # pyre-fixme[16]: Optional type has no attribute
                # `build_appliance_path`.
                opts.subvolume_on_disk.build_appliance_path
                / ANTLIR_DIR.strip_leading_slashes()
            )

            assert not subvol_antlir_dir.exists()
            subvol.run_as_root(
                [
                    *CP_CLONE_CMD,
                    build_appliance_antlir_dir,
                    subvol.path(),
                ]
            )

            # pyre-fixme[7]: Expected `_NspawnSetup` but got
            #  `Generator[antlir.subvol_utils.Subvol, None, None]`.
            yield subvol

            subvol.run_as_root(["rm", "-rf", subvol_antlir_dir])
