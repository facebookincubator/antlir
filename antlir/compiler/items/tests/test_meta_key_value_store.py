#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.compiler.items.meta_key_value_store import (
    load_meta_key_value_store_items,
    MetaKeyValueStoreItem,
)
from antlir.compiler.items.tests.common import (
    BaseItemTestCase,
    DUMMY_LAYER_OPTS,
    with_mocked_temp_volume_dir,
)

from antlir.compiler.requires_provides import (
    ProvidesKey,
    RequireKey,
    RequireUser,
)
from antlir.subvol_utils import TempSubvolumes


class MetaKeyValueStoreItemTestCase(BaseItemTestCase):
    def _setup_subvol(self, ts: TempSubvolumes):
        subvol = ts.create("meta_key_value_store")
        subvol.run_as_root(["mkdir", subvol.path(".meta")])
        return subvol

    def test_meta_key_value_store_provides(self) -> None:
        self._check_item(
            MetaKeyValueStoreItem(
                key="key",
                value="value",
                require_key="requires",
            ),
            {ProvidesKey(key="key")},
            {
                RequireKey(key="requires"),
                RequireUser("root"),
            },
        )

        self._check_item(
            MetaKeyValueStoreItem(key="key", value="value"),
            {ProvidesKey(key="key")},
            {RequireUser("root")},
        )

    @with_mocked_temp_volume_dir
    def test_meta_key_value_store_items(self) -> None:
        with TempSubvolumes() as ts:
            subvol = self._setup_subvol(ts)
            items = [
                MetaKeyValueStoreItem(
                    key="key1",
                    value="value1",
                ),
                MetaKeyValueStoreItem(
                    key="key2",
                    value="value2",
                    require_key="key1",
                ),
                MetaKeyValueStoreItem(
                    key="key3",
                    value="value3",
                    require_key="key2",
                ),
            ]

            for item in items:
                item.build(subvol, DUMMY_LAYER_OPTS)

            self.assertEqual(
                [
                    MetaKeyValueStoreItem(
                        key="key1",
                        value="value1",
                    ),
                    MetaKeyValueStoreItem(
                        key="key2",
                        value="value2",
                        require_key="key1",
                    ),
                    MetaKeyValueStoreItem(
                        key="key3",
                        value="value3",
                        require_key="key2",
                    ),
                ],
                load_meta_key_value_store_items(subvol),
            )

    @with_mocked_temp_volume_dir
    def test_install_duplicate_meta_key_value_error(self):
        with TempSubvolumes() as ts:
            subvol = self._setup_subvol(ts)

            with self.assertRaisesRegex(
                RuntimeError,
                "Key `key` is already installed and must be removed",
            ):
                item = MetaKeyValueStoreItem(
                    key="key",
                    value="value",
                )
                item.build(subvol, DUMMY_LAYER_OPTS)
                item.build(subvol, DUMMY_LAYER_OPTS)
