#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import pwd
from typing import Iterable

from antlir.bzl.genrule_layer import genrule_layer_t
from antlir.config import repo_config
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import (
    NspawnPluginArgs,
    PopenArgs,
    new_nspawn_opts,
)
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.nspawn_in_subvol.plugins.rpm import rpm_nspawn_plugins
from antlir.subvol_utils import Subvol

from .common import LayerOpts, PhaseOrder


class GenruleLayerItem(genrule_layer_t):
    def phase_order(self):
        return PhaseOrder.GENRULE_LAYER

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["GenruleLayerItem"], layer_opts: LayerOpts
    ):
        (item,) = items
        assert isinstance(item, GenruleLayerItem), item

        def builder(subvol: Subvol):
            c_opts = item.container_opts
            # We should not auto-create /logs in genrule layers.
            assert not c_opts.internal_only_logs_tmpfs

            maybe_protect_antlir = ()
            if not c_opts.internal_only_unprotect_antlir_dir:
                antlir_path = subvol.path("__antlir__")
                # Fail if `/` is not readable:
                if antlir_path.exists(raise_permission_error=True):
                    maybe_protect_antlir = ((antlir_path, "/__antlir__"),)

            opts = new_nspawn_opts(
                layer=subvol,
                snapshot=False,
                cmd=item.cmd,
                chdir=repo_config().repo_root
                if repo_config().artifacts_require_repo
                else Path("/"),
                bindmount_ro=(
                    # The command can never change `/.meta`.
                    (subvol.path("/.meta"), "/.meta"),
                    # Block changes to `/__antlir__`, except for the purpose
                    # of populating snapshot caches.
                    *maybe_protect_antlir,
                ),
                # Future: support the case where the in-container user DB
                # diverges from the out-of-container user DB.  And user NS.
                user=pwd.getpwnam(item.user),
                # Make sure we give nspawn the target -> outputs mapping
                targets_and_outputs=layer_opts.target_to_path,
                bind_repo_ro=item.bind_repo_ro,
                boot=item.boot,
            )
            run_nspawn(  # NB: stdout redirects to stderr by default
                opts,
                PopenArgs(),
                plugins=rpm_nspawn_plugins(
                    opts=opts,
                    plugin_args=NspawnPluginArgs(
                        serve_rpm_snapshots=c_opts.serve_rpm_snapshots,
                        shadow_proxied_binaries=c_opts.shadow_proxied_binaries,
                        shadow_paths=c_opts.shadow_paths,
                    ),
                ),
            )

        return builder
