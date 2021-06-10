#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Exactly one item must exist in this phase.  If none is specified by the
`.bzl` code, then `dep_graph.py` injects a `FilesystemRootItem`.
"""
from dataclasses import dataclass
from typing import Iterable

from antlir.fs_utils import META_FLAVOR_FILE, open_for_read_decompress
from antlir.subvol_utils import Subvol

from .common import ImageItem, LayerOpts, PhaseOrder, ensure_meta_dir_exists


# This checks to make sure that the parent layer of an layer has the same flavor
# as the flavor specified by the current layer.
def _check_parent_flavor(parent_subvol, flavor):
    flavor_path = parent_subvol.path(META_FLAVOR_FILE)
    if flavor_path.exists():
        subvol_flavor = flavor_path.read_text()
        assert subvol_flavor == flavor, (
            f"Parent subvol {parent_subvol.path()} flavor {subvol_flavor} "
            f"does not match provided flavor {flavor}."
        )


@dataclass(init=False, frozen=True)
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
            _check_parent_flavor(parent.subvol, layer_opts.flavor)
            # This assumes that the parent has everything mounted already.
            ensure_meta_dir_exists(subvol, layer_opts)

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
            ensure_meta_dir_exists(subvol, layer_opts)

        return builder


@dataclass(init=False, frozen=True)
class ReceiveSendstreamItem(ImageItem):
    source: str

    def phase_order(self):
        return PhaseOrder.MAKE_SUBVOL

    @classmethod
    def get_phase_builder(
        cls, items: Iterable["ReceiveSendstreamItem"], layer_opts: LayerOpts
    ):
        (item,) = items

        def builder(subvol: Subvol):
            with open_for_read_decompress(
                item.source
            ) as sendstream, subvol.receive(sendstream):
                pass

        return builder
