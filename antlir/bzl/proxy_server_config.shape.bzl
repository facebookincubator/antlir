# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

proxy_server_config_t = shape.shape(
    fbpkg_pkg_list = shape.list(target_t),  # @oss-disable This list is used only to buld dependencies.
)
