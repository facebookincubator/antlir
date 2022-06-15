# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(
    ":ensure_dirs_exist.bzl",
    "image_ensure_dirs_exist",
    "image_ensure_subdirs_exist",
)

image = struct(
    ensure_dirs_exist = image_ensure_dirs_exist,
    ensure_subdirs_exist = image_ensure_subdirs_exist,
)
