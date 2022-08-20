# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the feature.* macros."

load("//antlir/bzl:structs.bzl", "structs")
load("//antlir/bzl/image/feature:apt.bzl", "feature_apt_install", "feature_apt_remove_if_exists")
load("//antlir/bzl/image/feature:clone.bzl", "feature_clone")
load("//antlir/bzl/image/feature:ensure_dirs_exist.bzl", "feature_ensure_dirs_exist", "feature_ensure_subdirs_exist")
load("//antlir/bzl/image/feature:install.bzl", "feature_install", "feature_install_buck_runnable")
load("//antlir/bzl/image/feature:meta_key_value_store.bzl", "feature_meta_store", "feature_remove_meta_store")
load("//antlir/bzl/image/feature:mount.bzl", "feature_host_dir_mount", "feature_host_file_mount", "feature_layer_mount")
load("//antlir/bzl/image/feature:new.bzl", "feature_new")
load("//antlir/bzl/image/feature:remove.bzl", "feature_remove")
load("//antlir/bzl/image/feature:requires.bzl", "feature_requires")
load("//antlir/bzl/image/feature:rpms.bzl", "feature_rpms_install", "feature_rpms_remove_if_exists")
load("//antlir/bzl/image/feature:symlink.bzl", "feature_ensure_dir_symlink", "feature_ensure_file_symlink")
load("//antlir/bzl/image/feature:tarball.bzl", "feature_tarball")
load("//antlir/bzl/image/feature:usergroup.bzl", "feature_group_add", "feature_setup_standard_user", "feature_user_add")
load("//antlir/bzl2:use_buck2_macros.bzl", "use_buck2_macros")
load("//antlir/bzl2/feature:feature.bzl", feature_buck2 = "feature")
load("//antlir/bzl2/rpms:rpms.bzl", rpms_buck2 = "rpms")

feature_buck1 = struct(
    clone = feature_clone,
    ensure_dir_symlink = feature_ensure_dir_symlink,
    ensure_dirs_exist = feature_ensure_dirs_exist,
    ensure_file_symlink = feature_ensure_file_symlink,
    ensure_subdirs_exist = feature_ensure_subdirs_exist,
    group_add = feature_group_add,
    host_dir_mount = feature_host_dir_mount,
    host_file_mount = feature_host_file_mount,
    install = feature_install,
    install_buck_runnable = feature_install_buck_runnable,
    layer_mount = feature_layer_mount,
    new = feature_new,
    remove = feature_remove,
    requires = feature_requires,
    rpms_install = feature_rpms_install,
    rpms_remove_if_exists = feature_rpms_remove_if_exists,
    apt_install = feature_apt_install,
    apt_remove = feature_apt_remove_if_exists,
    setup_standard_user = feature_setup_standard_user,
    tarball = feature_tarball,
    user_add = feature_user_add,
    meta_store = feature_meta_store,
    remove_meta_store = feature_remove_meta_store,
)
feature_buck1_dict = structs.to_dict(feature_buck1)

rpms_buck2_dict = structs.to_dict(struct(
    rpms_install = rpms_buck2.install,
    rpms_remove_if_exists = rpms_buck2.remove_if_exists,
))

feature_buck2_dict = structs.to_dict(feature_buck2)
feature_buck2_dict.update(rpms_buck2_dict)

feature = struct(**({
    key: feature_buck2_dict[key] if key in feature_buck2_dict else feature_buck1_dict[key]
    for key in feature_buck1_dict
} if use_buck2_macros() else feature_buck1_dict))
