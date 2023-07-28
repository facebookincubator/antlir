# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl/feature:clone.bzl", "clone")
load("//antlir/antlir2/bzl/feature:ensure_dirs_exist.bzl", "ensure_dirs_exist", "ensure_subdirs_exist")
load("//antlir/antlir2/bzl/feature:extract.bzl", "extract_buck_binary", "extract_from_layer")
load("//antlir/antlir2/bzl/feature:feature.bzl", feature_new = "feature")
load("//antlir/antlir2/bzl/feature:genrule.bzl", "genrule")
load("//antlir/antlir2/bzl/feature:install.bzl", "install")
load("//antlir/antlir2/bzl/feature:metakv.bzl", "metakv_remove", "metakv_store")
load("//antlir/antlir2/bzl/feature:mount.bzl", "host_mount", "layer_mount")
load("//antlir/antlir2/bzl/feature:remove.bzl", "remove")
load("//antlir/antlir2/bzl/feature:requires.bzl", "requires")
load("//antlir/antlir2/bzl/feature:rpms.bzl", "rpms_install", "rpms_remove_if_exists", "rpms_upgrade")
load("//antlir/antlir2/bzl/feature:symlink.bzl", "ensure_dir_symlink", "ensure_file_symlink")
load("//antlir/antlir2/bzl/feature:tarball.bzl", "tarball")
load("//antlir/antlir2/bzl/feature:usergroup.bzl", "group_add", "user_add", "usermod")
# @oss-disable
# @oss-disable

feature = struct(
    clone = clone,
    ensure_dirs_exist = ensure_dirs_exist,
    ensure_subdirs_exist = ensure_subdirs_exist,
    extract_from_layer = extract_from_layer,
    extract_buck_binary = extract_buck_binary,
    new = feature_new,
    genrule = genrule,
    install = install,
    layer_mount = layer_mount,
    metakv_store = metakv_store,
    metakv_remove = metakv_remove,
    host_mount = host_mount,
    remove = remove,
    requires = requires,
    rpms_install = rpms_install,
    rpms_remove_if_exists = rpms_remove_if_exists,
    rpms_upgrade = rpms_upgrade,
    ensure_file_symlink = ensure_file_symlink,
    ensure_dir_symlink = ensure_dir_symlink,
    tarball = tarball,
    user_add = user_add,
    usermod = usermod,
    group_add = group_add,
    # @oss-disable
    # @oss-disable
)
