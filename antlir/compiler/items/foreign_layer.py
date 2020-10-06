#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import pwd
from dataclasses import dataclass
from typing import Iterable

import pydantic
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import (
    NspawnPluginArgs,
    PopenArgs,
    new_nspawn_opts,
)
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.nspawn_in_subvol.plugins.rpm import rpm_nspawn_plugins
from antlir.subvol_utils import Subvol

from .common import ImageItem, LayerOpts, PhaseOrder
from .foreign_layer_t import foreign_layer_t


# Future: this probably belongs in a container_opts library:
def _nspawn_plugin_args_from_container_opts_t(opts):
    return NspawnPluginArgs(
        serve_rpm_snapshots=opts.serve_rpm_snapshots,
        shadow_proxied_binaries=opts.shadow_proxied_binaries,
    )


class ForeignLayerItem(foreign_layer_t):
    @pydantic.validator("container_opts")
    def pathify(cls, opts):  # noqa: B902
        # Future: use `shape.path` to make this unnecessary.
        return opts.copy(
            update={
                "serve_rpm_snapshots": tuple(
                    Path(s) for s in opts.serve_rpm_snapshots
                )
            }
        )

    def phase_order(self):
        return PhaseOrder.FOREIGN_LAYER

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["ForeignLayerItem"], layer_opts: LayerOpts
    ):
        (item,) = items
        assert isinstance(item, ForeignLayerItem), item

        def builder(subvol: Subvol):
            antlir_path = subvol.path("__antlir__")
            # Use `.stat()`, not `.exists()`, to fail if `/` is not readable.
            try:
                os.stat(antlir_path)
                maybe_protect_antlir = ((antlir_path, "/__antlir__"),)
            except FileNotFoundError:
                maybe_protect_antlir = ()

            opts = new_nspawn_opts(
                layer=subvol,
                snapshot=False,
                cmd=item.cmd,
                bindmount_ro=(
                    # The command cannot change `/.meta` & `/__antlir__`
                    (subvol.path("/.meta"), "/.meta"),
                    *maybe_protect_antlir,
                ),
                # Future: support the case where the in-container user DB
                # diverges from the out-of-container user DB.  And user NS.
                user=pwd.getpwnam(item.user),
            )
            run_nspawn(  # NB: stdout redirects to stderr by default
                opts,
                PopenArgs(),
                plugins=rpm_nspawn_plugins(
                    opts=opts,
                    plugin_args=_nspawn_plugin_args_from_container_opts_t(
                        item.container_opts
                    ),
                ),
            )

        return builder
