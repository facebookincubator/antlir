# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

conf_t = shape.shape(
    search_domains = shape.list(str),
    nameservers = shape.list(str),
)
