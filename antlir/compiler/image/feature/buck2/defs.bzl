# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/compiler/image/feature/buck2:new.bzl", "feature_new")
load("//antlir/compiler/image/feature/buck2:remove.bzl", "feature_remove")
load("//antlir/compiler/image/feature/buck2:requires.bzl", "feature_requires")
load("//antlir/compiler/image/feature/buck2:symlink.bzl", "feature_ensure_dir_symlink", "feature_ensure_file_symlink")

feature = struct(
    new = feature_new,
    remove = feature_remove,
    requires = feature_requires,
    ensure_dir_symlink = feature_ensure_dir_symlink,
    ensure_file_symlink = feature_ensure_file_symlink,
)

# Remove when buck1 features aren't needed
feature_buck2 = feature
