# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the image_* macros."

load("//antlir/bzl/image_actions:clone.bzl", "image_clone")
load("//antlir/bzl/image_actions:ensure_dirs_exist.bzl", "image_ensure_dirs_exist", "image_ensure_subdirs_exist")
load("//antlir/bzl/image_actions:mount.bzl", "image_host_dir_mount", "image_host_file_mount", "image_layer_mount")
load("//antlir/bzl/image_actions:rpms.bzl", "image_rpms_install", "image_rpms_remove_if_exists")
load(":constants.bzl", "new_nevra")
load(":image_cpp_unittest.bzl", "image_cpp_unittest")
load(":image_genrule_layer.bzl", "image_genrule_layer")
load(":image_gpt.bzl", "image_gpt", "image_gpt_partition")
load(":image_layer.bzl", "image_layer")
load(":image_layer_alias.bzl", "image_layer_alias")
load(":image_layer_from_package.bzl", "image_layer_from_package")
load(":image_python_unittest.bzl", "image_python_unittest")
load(":image_rust_unittest.bzl", "image_rust_unittest")
load(":image_source.bzl", "image_source")
load(":image_test_rpm_names.bzl", "image_test_rpm_names")

image = struct(
    clone = image_clone,
    cpp_unittest = image_cpp_unittest,
    rust_unittest = image_rust_unittest,
    ensure_dirs_exist = image_ensure_dirs_exist,
    ensure_subdirs_exist = image_ensure_subdirs_exist,
    genrule_layer = image_genrule_layer,
    host_dir_mount = image_host_dir_mount,
    host_file_mount = image_host_file_mount,
    layer = image_layer,
    layer_alias = image_layer_alias,
    layer_from_package = image_layer_from_package,
    layer_mount = image_layer_mount,
    opts = struct,
    python_unittest = image_python_unittest,
    rpm = struct(nevra = new_nevra),
    rpms_install = image_rpms_install,
    rpms_remove_if_exists = image_rpms_remove_if_exists,
    source = image_source,
    test_rpm_names = image_test_rpm_names,
    gpt = image_gpt,
    gpt_partition = image_gpt_partition,
)
