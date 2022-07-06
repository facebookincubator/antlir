# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":install.bzl", "feature_install", "feature_install_buck_runnable")
load(":new.bzl", "feature_new")
load(":remove.bzl", "feature_remove")
load(":requires.bzl", "feature_requires")
load(
    ":symlink.bzl",
    "feature_ensure_dir_symlink",
    "feature_ensure_file_symlink",
)
load(":tarball.bzl", "feature_tarball")
load(
    ":usergroup.bzl",
    "feature_group_add",
    "feature_setup_standard_user",
    "feature_user_add",
)

feature = struct(
    install = feature_install,
    install_buck_runnable = feature_install_buck_runnable,
    new = feature_new,
    remove = feature_remove,
    requires = feature_requires,
    ensure_dir_symlink = feature_ensure_dir_symlink,
    ensure_file_symlink = feature_ensure_file_symlink,
    group_add = feature_group_add,
    user_add = feature_user_add,
    setup_standard_user = feature_setup_standard_user,
    tarball = feature_tarball,
)
