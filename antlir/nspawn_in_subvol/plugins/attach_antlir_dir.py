# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from contextlib import contextmanager

from antlir.fs_utils import ANTLIR_DIR, CP_CLONE_CMD
from antlir.nspawn_in_subvol.args import PopenArgs, _NspawnOpts
from antlir.nspawn_in_subvol.plugin_hooks import (
    _NspawnSetup,
    _NspawnSetupCtxMgr,
)

from . import NspawnPlugin


class AttachAntlirDir(NspawnPlugin):
    @contextmanager
    def wrap_setup(
        self,
        setup_ctx: _NspawnSetupCtxMgr,
        opts: _NspawnOpts,
        popen_args: PopenArgs,
    ) -> _NspawnSetup:
        with setup_ctx(opts, popen_args) as setup:
            subvol = setup.subvol
            subvol_antlir_dir = subvol.path(ANTLIR_DIR)
            build_appliance_antlir_dir = (
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

            yield setup

            subvol.run_as_root(["rm", "-rf", subvol_antlir_dir])
