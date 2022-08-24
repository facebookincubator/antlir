# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

meta_key_value_store_item_t = shape.shape(
    key = str,
    value = str,
    require_keys = shape.field(shape.list(str), default = []),
    store_if_not_exists = shape.field(bool, default = False),
)

remove_meta_key_value_store_item_t = shape.shape(
    key = str,
)
