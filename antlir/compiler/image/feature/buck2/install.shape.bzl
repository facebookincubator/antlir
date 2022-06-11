# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:mode.shape.bzl", "mode_t")
load("//antlir/bzl:shape.bzl", "shape")
load(":image_source.shape.bzl", "image_source_t")

install_files_t = shape.shape(
    dest = shape.path,
    source = image_source_t,
    mode = shape.field(mode_t, optional = True),
    user_group = shape.field(str, optional = True),
    dir_mode = shape.field(mode_t, optional = True),
    exe_mode = shape.field(mode_t, optional = True),
    data_mode = shape.field(mode_t, optional = True),
)
