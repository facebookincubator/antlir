# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/feature/clone.bzl", "clone")
load("//antlir/antlir2/feature/ensure_dirs_exist.bzl", "ensure_dirs_exist", "ensure_subdirs_exist")
load("//antlir/antlir2/feature/extract.bzl", "extract_buck_binary", "extract_from_layer")
load("//antlir/antlir2/feature/feature.bzl", feature_new = "feature")
load("//antlir/antlir2/feature/genrule.bzl", "genrule")
load("//antlir/antlir2/feature/install.bzl", "install")
load("//antlir/antlir2/feature/meta_kv.bzl", "meta_remove", "meta_store")
load("//antlir/antlir2/feature/mount.bzl", "host_mount", "layer_mount")
load("//antlir/antlir2/feature/remove.bzl", "remove")
load("//antlir/antlir2/feature/rpms.bzl", "rpms_install", "rpms_remove_if_exists")
load("//antlir/antlir2/feature/symlink.bzl", "ensure_dir_symlink", "ensure_file_symlink")
load("//antlir/antlir2/feature/tarball.bzl", "tarball")
load("//antlir/antlir2/feature/usergroup.bzl", "group_add", "user_add", "usermod")

feature = struct(
    clone = clone,
    ensure_dirs_exist = ensure_dirs_exist,
    ensure_subdirs_exist = ensure_subdirs_exist,
    extract_from_layer = extract_from_layer,
    extract_buck_binary = extract_buck_binary,
    new = feature_new,
    genrule = genrule,
    install = install,
    meta_store = meta_store,
    meta_remove = meta_remove,
    layer_mount = layer_mount,
    host_mount = host_mount,
    remove = remove,
    rpms_install = rpms_install,
    rpms_remove_if_exists = rpms_remove_if_exists,
    ensure_file_symlink = ensure_file_symlink,
    ensure_dir_symlink = ensure_dir_symlink,
    tarball = tarball,
    user_add = user_add,
    usermod = usermod,
    group_add = group_add,
)
