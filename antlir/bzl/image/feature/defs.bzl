# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the feature.* macros."

load("//antlir/bzl/image/feature:install.bzl", "feature_install", "feature_install_buck_runnable")
load("//antlir/bzl/image/feature:new.bzl", "feature_new")
load("//antlir/bzl/image/feature:remove.bzl", "feature_remove")
load("//antlir/bzl/image/feature:requires.bzl", "feature_requires")
load("//antlir/bzl/image/feature:symlink.bzl", "feature_ensure_dir_symlink", "feature_ensure_file_symlink")
load("//antlir/bzl/image/feature:tarball.bzl", "feature_tarball")
load("//antlir/bzl/image/feature:usergroup.bzl", "feature_group_add", "feature_user_add")

feature = struct(
    requires = feature_requires,
    group_add = feature_group_add,
    install = feature_install,
    install_buck_runnable = feature_install_buck_runnable,
    new = feature_new,
    remove = feature_remove,
    user_add = feature_user_add,
    tarball = feature_tarball,
    ensure_dir_symlink = feature_ensure_dir_symlink,
    ensure_file_symlink = feature_ensure_file_symlink,
)
