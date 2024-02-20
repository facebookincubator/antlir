# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the image_* macros."

load(":constants.bzl", "new_nevra")
load(":image_cpp_unittest.bzl", "image_cpp_unittest")
load(":image_genrule_layer.bzl", "image_genrule_layer")
load(":image_gpt.bzl", "image_gpt", "image_gpt_partition")
load(":image_layer.bzl", "image_layer")
load(":image_python_unittest.bzl", "image_python_unittest")
load(":image_rust_unittest.bzl", "image_rust_unittest")
load(":image_source.bzl", "image_source")
load(":image_test_rpm_names.bzl", "image_test_rpm_names")

image = struct(
    cpp_unittest = image_cpp_unittest,
    rust_unittest = image_rust_unittest,
    genrule_layer = image_genrule_layer,
    layer = image_layer,
    opts = struct,
    python_unittest = image_python_unittest,
    rpm = struct(nevra = new_nevra),
    source = image_source,
    test_rpm_names = image_test_rpm_names,
    gpt = image_gpt,
    gpt_partition = image_gpt_partition,
)
