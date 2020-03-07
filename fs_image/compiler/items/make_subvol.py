#!/usr/bin/env python3
'''
Exactly one item must exist in this phase.  If none is specified by the
`.bzl` code, then `dep_graph.py` injects a `FilesystemRootItem`.
'''
from dataclasses import dataclass
from typing import Iterable

from fs_image.fs_utils import open_for_read_decompress
from subvol_utils import Subvol

from .common import ensure_meta_dir_exists, ImageItem, LayerOpts, PhaseOrder
from .mount_utils import clone_mounts


@dataclass(init=False, frozen=True)
class ParentLayerItem(ImageItem):
    subvol: Subvol

    def phase_order(self):
        return PhaseOrder.MAKE_SUBVOL

    @classmethod
    def get_phase_builder(
        cls, items: Iterable['ParentLayerItem'], layer_opts: LayerOpts,
    ):
        parent, = items
        assert isinstance(parent, ParentLayerItem), parent

        def builder(subvol: Subvol):
            subvol.snapshot(parent.subvol)
            # This assumes that the parent has everything mounted already.
            clone_mounts(parent.subvol, subvol)
            ensure_meta_dir_exists(subvol, layer_opts)

        return builder


@dataclass(init=False, frozen=True)
class FilesystemRootItem(ImageItem):
    'A simple item to endow parent-less layers with a standard-permissions /'

    def phase_order(self):
        return PhaseOrder.MAKE_SUBVOL

    @classmethod
    def get_phase_builder(
        cls, items: Iterable['FilesystemRootItem'], layer_opts: LayerOpts,
    ):
        parent, = items
        assert isinstance(parent, FilesystemRootItem), parent

        def builder(subvol: Subvol):
            subvol.create()
            # Guarantee standard / permissions.  This could be a setting,
            # but in practice, probably any other choice would be wrong.
            subvol.run_as_root(['chmod', '0755', subvol.path()])
            subvol.run_as_root(['chown', 'root:root', subvol.path()])
            ensure_meta_dir_exists(subvol, layer_opts)

        return builder


@dataclass(init=False, frozen=True)
class ReceiveSendstreamItem(ImageItem):
    source: str

    def phase_order(self):
        return PhaseOrder.MAKE_SUBVOL

    @classmethod
    def get_phase_builder(
        cls, items: Iterable['ReceiveSendstreamItem'], layer_opts: LayerOpts,
    ):
        item, = items

        def builder(subvol: Subvol):
            with open_for_read_decompress(item.source) as sendstream, \
                    subvol.receive(sendstream):
                pass

        return builder
