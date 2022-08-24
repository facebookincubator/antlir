#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.compiler.items.common import PhaseOrder
from antlir.compiler.items.meta_key_value_store import (
    load_meta_key_value_store_items,
    MetaKeyValueStoreItem,
    RemoveMetaKeyValueStoreItem,
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

    def test_remove_runtime_config_phase_order(self) -> None:
        self.assertEqual(
            PhaseOrder.REMOVE_META_KEY_VALUE_STORE,
            RemoveMetaKeyValueStoreItem(key="key").phase_order(),
        )

    def test_meta_key_value_store_provides(self) -> None:
        self._check_item(
            MetaKeyValueStoreItem(
                key="key",
                value="value",
                require_keys=["require1", "require2"],
            ),
            {ProvidesKey(key="key")},
            {
                RequireKey(key="require1"),
                RequireKey(key="require2"),
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
                    require_keys=("key1",),
                ),
                MetaKeyValueStoreItem(
                    key="key3",
                    value="value3",
                    require_keys=("key2",),
                ),
            ]

            for item in items:
                item.build(subvol, DUMMY_LAYER_OPTS)
            RemoveMetaKeyValueStoreItem.get_phase_builder(
                [RemoveMetaKeyValueStoreItem(key="key2")],
                DUMMY_LAYER_OPTS,
            )(subvol)

            self.assertEqual(
                [
                    MetaKeyValueStoreItem(
                        key="key1",
                        value="value1",
                    ),
                    MetaKeyValueStoreItem(
                        key="key3",
                        value="value3",
                        require_keys=("key2",),
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

    @with_mocked_temp_volume_dir
    def test_remove_nonexistent_key_value_error(self):
        with TempSubvolumes() as ts:
            subvol = self._setup_subvol(ts)

            with self.assertRaisesRegex(
                RuntimeError,
                "No key value pairs were stored so none could be removed",
            ):
                RemoveMetaKeyValueStoreItem.get_phase_builder(
                    [RemoveMetaKeyValueStoreItem(key="key")],
                    DUMMY_LAYER_OPTS,
                )(subvol)

    @with_mocked_temp_volume_dir
    def test_remove_incorrect_key_value_error(self):
        with TempSubvolumes() as ts:
            subvol = self._setup_subvol(ts)
            MetaKeyValueStoreItem(
                key="key",
                value="value",
            ).build(subvol, DUMMY_LAYER_OPTS)

            with self.assertRaisesRegex(
                RuntimeError,
                "Key `key2` does not exist and cannot be removed",
            ):
                RemoveMetaKeyValueStoreItem.get_phase_builder(
                    [RemoveMetaKeyValueStoreItem(key="key2")],
                    DUMMY_LAYER_OPTS,
                )(subvol)

    @with_mocked_temp_volume_dir
    def test_store_if_not_exists(self) -> None:
        with TempSubvolumes() as ts:
            subvol = self._setup_subvol(ts)
            items = [
                MetaKeyValueStoreItem(
                    key="key1",
                    value="value1",
                ),
                MetaKeyValueStoreItem(
                    key="key1",
                    value="value2",
                    store_if_not_exists=True,
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
                ],
                load_meta_key_value_store_items(subvol),
            )
