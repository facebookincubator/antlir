# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

boot_loader_entry_t = shape.shape(
    title = str,
    kernel = str,
    initrds = shape.list(str),
    options = shape.list(str),
)
