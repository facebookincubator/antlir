#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import functools
import os
import pwd

from dataclasses import dataclass
from typing import Iterable

from fs_image.fs_utils import Path
from fs_image.nspawn_in_subvol.args import new_nspawn_opts, PopenArgs
from fs_image.nspawn_in_subvol.non_booted import run_non_booted_nspawn
from fs_image.nspawn_in_subvol.inject_repo_servers import (
    nspawn_plugin_to_inject_repo_servers,
)
from fs_image.subvol_utils import Subvol

from .common import ImageItem, LayerOpts, PhaseOrder


@dataclass(init=False, frozen=True)
class ForeignLayerItem(ImageItem):
    # IMPORTANT: Be very cautious about adding keys here, specifically
    # rejecting any options that might compromise determinism / hermeticity.
    # Foreign layers effectively run arbitrary code, so we should never
    # allow access to the network, nor read-write access to files outside of
    # the layer.  If you need something from the foreign layer, build it,
    # then reach into it with `image.source`.
    cmd: Iterable[str]
    user: str
    serve_rpm_snapshots: Iterable[str]

    # This type-checking isn't strictly required, but it helps to fail fast.
    def customize_fields(kwargs):  # noqa: B902
        cmd = kwargs.pop('cmd')
        assert all(isinstance(c, (str, bytes)) for c in cmd), cmd
        kwargs['cmd'] = tuple(cmd)

        assert isinstance(kwargs['user'], str), kwargs['user']

        kwargs['serve_rpm_snapshots'] = tuple(
            Path(s) for s in kwargs.pop('serve_rpm_snapshots')
        )

    def phase_order(self):
        return PhaseOrder.FOREIGN_LAYER

    @classmethod
    def get_phase_builder(
        cls, items: Iterable['ForeignLayerItem'], layer_opts: LayerOpts,
    ):
        item, = items
        assert isinstance(item, ForeignLayerItem), item

        def builder(subvol: Subvol):
            fs_image_path = subvol.path('__fs_image__')
            # Use `.stat()`, not `.exists()`, to fail if `/` is not readable.
            try:
                os.stat(fs_image_path)
                maybe_protect_fs_image = ((fs_image_path, '/__fs_image__'),)
            except FileNotFoundError:
                maybe_protect_fs_image = ()

            run_non_booted_nspawn(  # NB: stdout redirects to stderr by default
                new_nspawn_opts(
                    layer=subvol,
                    snapshot=False,
                    cmd=item.cmd,
                    bindmount_ro=(
                        # The command cannot change `/meta` & `/__fs_image__`
                        (subvol.path('/meta'), '/meta'),
                        *maybe_protect_fs_image,
                    ),
                    # Future: support the case where the in-container user DB
                    # diverges from the out-of-container user DB.  And user NS.
                    user=pwd.getpwnam(item.user),
                ),
                PopenArgs(),
                plugins=[nspawn_plugin_to_inject_repo_servers(
                    item.serve_rpm_snapshots,
                )],
            )

        return builder
