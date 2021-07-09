# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the image_* macros."

load("//antlir/bzl/image_actions:clone.bzl", "image_clone")
load("//antlir/bzl/image_actions:ensure_dirs_exist.bzl", "image_ensure_dirs_exist", "image_ensure_subdirs_exist")
load("//antlir/bzl/image_actions:install.bzl", "image_install", "image_install_buck_runnable")
load("//antlir/bzl/image_actions:mount.bzl", "image_host_dir_mount", "image_host_file_mount", "image_layer_mount")
load("//antlir/bzl/image_actions:remove.bzl", "image_remove")
load("//antlir/bzl/image_actions:rpms.bzl", "image_rpms_install", "image_rpms_remove_if_exists")
load("//antlir/bzl/image_actions:symlink.bzl", "image_ensure_dir_symlink", "image_ensure_file_symlink")
load("//antlir/bzl/image_actions:tarball.bzl", "image_tarball")
load(":constants.bzl", "new_nevra")
load(":image_cpp_unittest.bzl", "image_cpp_unittest")
load(":image_genrule_layer.bzl", "image_genrule_layer")
load(":image_gpt.bzl", "image_gpt", "image_gpt_partition")
load(":image_layer.bzl", "image_layer")
load(":image_layer_alias.bzl", "image_layer_alias")
load(":image_layer_from_package.bzl", "image_layer_from_package")
load(":image_package.bzl", "image_package")
load(":image_packaged_layer.bzl", "image_packaged_layer")
load(":image_python_unittest.bzl", "image_python_unittest")
load(":image_rust_unittest.bzl", "image_rust_unittest")
load(":image_source.bzl", "image_source")
load(":image_test_rpm_names.bzl", "image_test_rpm_names")

image = struct(
    clone = image_clone,
    cpp_unittest = image_cpp_unittest,
    rust_unittest = image_rust_unittest,
    ensure_dir_symlink = image_ensure_dir_symlink,
    ensure_dirs_exist = image_ensure_dirs_exist,
    ensure_file_symlink = image_ensure_file_symlink,
    ensure_subdirs_exist = image_ensure_subdirs_exist,
    genrule_layer = image_genrule_layer,
    host_dir_mount = image_host_dir_mount,
    host_file_mount = image_host_file_mount,
    install = image_install,
    install_buck_runnable = image_install_buck_runnable,
    layer = image_layer,
    layer_alias = image_layer_alias,
    layer_from_package = image_layer_from_package,
    layer_mount = image_layer_mount,
    opts = struct,
    package = image_package,
    packaged_layer = image_packaged_layer,
    python_unittest = image_python_unittest,
    remove = image_remove,
    rpm = struct(nevra = new_nevra),
    rpms_install = image_rpms_install,
    rpms_remove_if_exists = image_rpms_remove_if_exists,
    source = image_source,
    tarball = image_tarball,
    test_rpm_names = image_test_rpm_names,
    gpt = image_gpt,
    gpt_partition = image_gpt_partition,
)
