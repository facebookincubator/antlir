# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":install.bzl", "feature_install")
load(":new.bzl", "feature_new")
load(":remove.bzl", "feature_remove")
load(":requires.bzl", "feature_requires")
load(
    ":symlink.bzl",
    "feature_ensure_dir_symlink",
    "feature_ensure_file_symlink",
)
load(":tarball.bzl", "feature_tarball")
load(":usergroup.bzl", "feature_group_add", "feature_user_add")

feature = struct(
    install = feature_install,
    new = feature_new,
    remove = feature_remove,
    requires = feature_requires,
    ensure_dir_symlink = feature_ensure_dir_symlink,
    ensure_file_symlink = feature_ensure_file_symlink,
    group_add = feature_group_add,
    user_add = feature_user_add,
    tarball = feature_tarball,
)
