# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    "//antlir/compiler/image/buck2:image_genrule_layer.bzl",
    "image_genrule_layer",
)
load("//antlir/compiler/image/buck2:image_layer.bzl", "image_layer")
load(":clone.bzl", "image_clone")
load(
    ":ensure_dirs_exist.bzl",
    "image_ensure_dirs_exist",
    "image_ensure_subdirs_exist",
)
load(":rpms.bzl", "image_rpms_install")

image = struct(
    ensure_dirs_exist = image_ensure_dirs_exist,
    ensure_subdirs_exist = image_ensure_subdirs_exist,
    clone = image_clone,
    layer = image_layer,
    rpms_install = image_rpms_install,
    genrule_layer = image_genrule_layer,
)
