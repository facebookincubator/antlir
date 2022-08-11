# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
from typing import List, Optional

from antlir.bzl.image.feature.meta_key_value_store import (
    meta_key_value_store_item_t,
)

from antlir.compiler.items.common import ImageItem, LayerOpts
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
    require_key: Optional[str]

    def provides(self):
        yield ProvidesKey(self.key)

    def requires(self):
        if self.require_key:
            yield RequireKey(self.require_key)
        # Make sure this item is after PhasesProvide
        yield RequireUser("root")

    def build(self, subvol: Subvol, layer_opts: LayerOpts) -> None:
        items = []
        if subvol.path(META_KEY_VALUE_STORE_FILE).exists():
            items = load_meta_key_value_store_items(subvol)
        if _contains(items, self.key):
            raise RuntimeError(
                f"Key `{self.key}` is already installed " "and must be removed"
            )
        items.append(self)
        store_meta_key_value_store_items(subvol, items)


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
