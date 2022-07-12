# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

ensure_subdirs_exist_t = shape.shape(
    into_dir = str,
    subdirs_to_create = str,
    mode = shape.field(int, default = 0o755),
    user = shape.field(str, default = "root"),
    group = shape.field(str, default = "root"),
)
