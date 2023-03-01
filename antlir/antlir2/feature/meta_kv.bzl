# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def meta_store(
        *,
        key: str.type,
        value: str.type,
        require_keys: [str.type] = [],
        store_if_not_exists: bool.type = False) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "meta_key_value_store",
        kwargs = {
            "key": key,
            "require_keys": require_keys,
            "store_if_not_exists": store_if_not_exists,
            "value": value,
        },
    )

def meta_remove(
        *,
        key: str.type) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "meta_key_value_remove",
        kwargs = {
            "key": key,
        },
    )

def meta_store_to_json(
        key: str.type,
        value: str.type,
        require_keys: [str.type],
        store_if_not_exists: bool.type) -> {str.type: ""}:
    return {
        "key": key,
        "require_keys": require_keys,
        "store_if_not_exists": store_if_not_exists,
        "value": value,
    }

def meta_remove_to_json(key: str.type) -> {str.type: ""}:
    return {
        "key": key,
    }