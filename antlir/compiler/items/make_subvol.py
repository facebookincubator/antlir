#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Exactly one item must exist in this phase.  If none is specified by the
`.bzl` code, then `dep_graph.py` injects a `FilesystemRootItem`.
"""
from dataclasses import dataclass
from typing import Iterable

from antlir.compiler.items.common import (
    ImageItem,
    LayerOpts,
    PhaseOrder,
    setup_meta_dir,
)

from antlir.fs_utils import META_FLAVOR_FILE, open_for_read_decompress
from antlir.subvol_utils import Subvol

# This checks to make sure that the parent layer of an layer has the same flavor
# as the flavor specified by the current layer.
def _check_parent_flavor(parent_subvol, flavor) -> None:
    flavor_path = parent_subvol.path(META_FLAVOR_FILE)
    if flavor_path.exists():
        subvol_flavor = flavor_path.read_text()
        assert subvol_flavor == flavor, (
            f"Parent subvol {parent_subvol.path()} flavor {subvol_flavor} "
            f"does not match provided flavor {flavor}."
        )


@dataclass(init=False, frozen=True)
# pyre-fixme[13]: Attribute `subvol` is never initialized.
class ParentLayerItem(ImageItem):
    subvol: Subvol

    def phase_order(self):
        return PhaseOrder.MAKE_SUBVOL

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["ParentLayerItem"], layer_opts: LayerOpts
    ):
        (parent,) = items
        assert isinstance(parent, ParentLayerItem), parent

        def builder(subvol: Subvol):
            subvol.snapshot(parent.subvol)
            if not layer_opts.unsafe_bypass_flavor_check:
                _check_parent_flavor(parent.subvol, layer_opts.flavor)
            # This assumes that the parent has everything mounted already.
            setup_meta_dir(subvol, layer_opts)

        return builder


@dataclass(init=False, frozen=True)
class FilesystemRootItem(ImageItem):
    "A simple item to endow parent-less layers with a standard-permissions /"

    def phase_order(self):
        return PhaseOrder.MAKE_SUBVOL

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["FilesystemRootItem"], layer_opts: LayerOpts
    ):
        (parent,) = items
        assert isinstance(parent, FilesystemRootItem), parent

        def builder(subvol: Subvol):
            subvol.create()
            # Guarantee standard / permissions.  This could be a setting,
            # but in practice, probably any other choice would be wrong.
            subvol.run_as_root(["chmod", "0755", subvol.path()])
            subvol.run_as_root(["chown", "root:root", subvol.path()])
            setup_meta_dir(subvol, layer_opts)

        return builder


@dataclass(init=False, frozen=True)
# pyre-fixme[13]: Attribute `format` is never initialized.
# pyre-fixme[13]: Attribute `source` is never initialized.
class LayerFromPackageItem(ImageItem):
    format: str
    source: str

    def phase_order(self):
        return PhaseOrder.MAKE_SUBVOL

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["LayerFromPackageItem"], layer_opts: LayerOpts
    ):
        (item,) = items

        def builder(subvol: Subvol):
            if item.format in ["sendstream", "sendstream.v2"]:
                with open_for_read_decompress(
                    item.source
                ) as sendstream, subvol.receive(sendstream):
                    pass
            else:
                raise Exception(
                    f"Unsupported format {item.format} for layer from package."
                )
            # The normal item contract is "leave the SV read-write" -- the
            # compiler will mark it RO at the end.  This is important e.g.
            # for setting `/.meta/flavor`.
            subvol.set_readonly(False)
            setup_meta_dir(subvol, layer_opts)

        return builder
