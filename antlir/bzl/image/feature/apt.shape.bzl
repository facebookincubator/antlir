# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

action_t = shape.enum("install", "remove_if_exists")

apt_action_item_t = shape.shape(
    package_names = shape.list(str),
    action = action_t,
)
