# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features/clone:clone.bzl", "clone")
load("//antlir/antlir2/features/ensure_dir_exists:ensure_dir_exists.bzl", "ensure_dirs_exist", "ensure_subdirs_exist")
load("//antlir/antlir2/features/extract:extract.bzl", "extract_buck_binary", "extract_from_layer")
# @oss-disable
# @oss-disable
# @oss-disable
load("//antlir/antlir2/features/genrule:genrule.bzl", "genrule")
load("//antlir/antlir2/features/group:group.bzl", "group_add")
load("//antlir/antlir2/features/install:install.bzl", "install", "install_text")
load("//antlir/antlir2/features/mknod:mknod.bzl", "mknod")
load("//antlir/antlir2/features/mount:mount.bzl", "host_mount", "layer_mount")
load("//antlir/antlir2/features/remove:remove.bzl", "remove")
load("//antlir/antlir2/features/requires:requires.bzl", "requires")
load("//antlir/antlir2/features/rpm:rpm.bzl", "rpms_install", "rpms_remove", "rpms_remove_if_exists", "rpms_upgrade")
load("//antlir/antlir2/features/symlink:symlink.bzl", "ensure_dir_symlink", "ensure_file_symlink")
load("//antlir/antlir2/features/tarball:tarball.bzl", "tarball")
load("//antlir/antlir2/features/user:user.bzl", "standard_user", "user_add")
load("//antlir/antlir2/features/usermod:usermod.bzl", "usermod")
load(":feature.bzl", feature_new = "feature")

feature = struct(
    clone = clone,
    ensure_dirs_exist = ensure_dirs_exist,
    ensure_subdirs_exist = ensure_subdirs_exist,
    extract_from_layer = extract_from_layer,
    extract_buck_binary = extract_buck_binary,
    new = feature_new,
    genrule = genrule,
    install = install,
    install_text = install_text,
    layer_mount = layer_mount,
    host_mount = host_mount,
    remove = remove,
    requires = requires,
    mknod = mknod,
    rpms_install = rpms_install,
    rpms_remove = rpms_remove,
    rpms_remove_if_exists = rpms_remove_if_exists,
    rpms_upgrade = rpms_upgrade,
    ensure_file_symlink = ensure_file_symlink,
    ensure_dir_symlink = ensure_dir_symlink,
    tarball = tarball,
    user_add = user_add,
    usermod = usermod,
    group_add = group_add,
    standard_user = standard_user,
    # @oss-disable
    # @oss-disable
    # @oss-disable
)
