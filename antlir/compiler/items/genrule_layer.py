#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import pwd
from typing import Iterable

from antlir.bzl.genrule_layer import genrule_layer_t
from antlir.common import not_none
from antlir.compiler.subvolume_on_disk import SubvolumeOnDisk
from antlir.config import repo_config
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import (
    AttachAntlirDirMode,
    new_nspawn_opts,
    NspawnPluginArgs,
    PopenArgs,
)
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.nspawn_in_subvol.plugins.repo_plugins import repo_nspawn_plugins
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
                if item.bind_repo_ro
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
                subvolume_on_disk=SubvolumeOnDisk.from_subvolume_path(
                    subvol.path(),
                    layer_opts.subvolumes_dir,
                    not_none(layer_opts.build_appliance).path(),
                )
                if c_opts.attach_antlir_dir
                else None,
            )
            run_nspawn(  # NB: stdout redirects to stderr by default
                opts,
                PopenArgs(),
                plugins=repo_nspawn_plugins(
                    opts=opts,
                    plugin_args=NspawnPluginArgs(
                        serve_rpm_snapshots=c_opts.serve_rpm_snapshots,
                        shadow_proxied_binaries=c_opts.shadow_proxied_binaries,
                        shadow_paths=c_opts.shadow_paths,
                        run_proxy_server=c_opts.run_proxy_server,
                        fbpkg_db_path=c_opts.fbpkg_db_path,
                        attach_antlir_dir=(
                            AttachAntlirDirMode.EXPLICIT_ON
                            if c_opts.attach_antlir_dir
                            else AttachAntlirDirMode.OFF
                        ),
                    ),
                ),
            )

        return builder
