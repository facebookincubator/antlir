# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
from typing import Iterable, List, Tuple

from antlir.bzl.image.feature.meta_key_value_store import (
    meta_key_value_store_item_t,
    remove_meta_key_value_store_item_t,
)

from antlir.compiler.items.common import ImageItem, LayerOpts, PhaseOrder
from antlir.compiler.requires_provides import (
    ProvidesKey,
    RequireKey,
    RequireUser,
)
from antlir.fs_utils import META_DIR
from antlir.subvol_utils import Subvol

from pydantic import BaseModel

# The name of the directory that contains information
# about the runtime configuration of the layer
META_KEY_VALUE_STORE_FILE = META_DIR / "key_value_store"


# pyre-fixme[13]: Attribute `key`, `value` is never initialized.
class MetaKeyValueStoreItem(meta_key_value_store_item_t, ImageItem):
    key: str
    value: str
    require_keys: Tuple[str, ...] = ()
    store_if_not_exists: bool = False

    def provides(self):
        yield ProvidesKey(self.key)

    def requires(self):
        for key in self.require_keys:
            yield RequireKey(key)
        # Make sure this item is after PhasesProvide
        yield RequireUser("root")

    def build(self, subvol: Subvol, layer_opts: LayerOpts) -> None:
        items = []
        if subvol.path(META_KEY_VALUE_STORE_FILE).exists():
            items = load_meta_key_value_store_items(subvol)
        if not self.store_if_not_exists:
            # Since we cannot guarantee ordering of items,
            # items with store_if_not_exists=False will remove
            # all previous items with store_if_not_exists=True
            items = [
                item
                for item in items
                if not (item.key == self.key and item.store_if_not_exists)
            ]

        if not _contains(items, self.key):
            items.append(self)
        elif not self.store_if_not_exists:
            raise RuntimeError(
                f"Key `{self.key}` is already installed " "and must be removed"
            )
        store_meta_key_value_store_items(subvol, items)


# pyre-fixme[13]: Attribute `key` is never initialized.
class RemoveMetaKeyValueStoreItem(
    remove_meta_key_value_store_item_t, ImageItem
):
    key: str

    def phase_order(self):
        return PhaseOrder.REMOVE_META_KEY_VALUE_STORE

    @classmethod
    def get_phase_builder(
        cls,
        items: Iterable["RemoveMetaKeyValueStoreItem"],
        layer_opts: LayerOpts,
    ):
        def builder(subvol: Subvol):
            if not subvol.path(META_KEY_VALUE_STORE_FILE).exists():
                raise RuntimeError(
                    "No key value pairs were stored so none could be removed",
                )

            stored_items = load_meta_key_value_store_items(subvol)

            keys_to_remove = {item.key for item in items}
            stored_keys = {item.key for item in stored_items}

            for key in keys_to_remove:
                if key not in stored_keys:
                    raise RuntimeError(
                        f"Key `{key}` does not exist and cannot be removed"
                    )

            store_meta_key_value_store_items(
                subvol,
                [
                    item
                    for item in stored_items
                    if item.key not in keys_to_remove
                ],
            )

        return builder


# pyre-fixme[13]: Attribute `items` is never initialized.
class MetaKeyValueStoreItems(BaseModel):
    items: List[MetaKeyValueStoreItem]


def load_meta_key_value_store_items(
    subvol: Subvol,
) -> List[MetaKeyValueStoreItem]:
    return list(
        MetaKeyValueStoreItems(
            **json.loads(subvol.read_path_text(META_KEY_VALUE_STORE_FILE))
        ).items
    )


def store_meta_key_value_store_items(
    subvol: Subvol, items: List[meta_key_value_store_item_t]
) -> None:
    subvol.overwrite_path_as_root(
        META_KEY_VALUE_STORE_FILE, MetaKeyValueStoreItems(items=items).json()
    )


def _contains(items: List[MetaKeyValueStoreItem], key: str) -> bool:
    return key in {item.key for item in items}
