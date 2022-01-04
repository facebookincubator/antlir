# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:mode.shape.bzl", "mode_t")
load("//antlir/bzl:shape.bzl", "shape")

ensure_subdirs_exist_t = shape.shape(
    into_dir = str,
    subdirs_to_create = str,
    mode = shape.field(mode_t, optional = True),
    user_group = shape.field(str, optional = True),
)
