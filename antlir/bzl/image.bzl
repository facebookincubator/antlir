# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the image_* macros."

load("//antlir/bzl2:use_buck2_macros.bzl", "use_buck2_macros")
load("//antlir/bzl2/layer:layer.bzl", layer_buck2 = "layer")
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
load(":structs.bzl", "structs")

image_buck1 = struct(
    cpp_unittest = image_cpp_unittest,
    rust_unittest = image_rust_unittest,
    genrule_layer = image_genrule_layer,
    layer = image_layer,
    layer_alias = image_layer_alias,
    layer_from_package = image_layer_from_package,
    opts = struct,
    python_unittest = image_python_unittest,
    rpm = struct(nevra = new_nevra),
    source = image_source,
    test_rpm_names = image_test_rpm_names,
    gpt = image_gpt,
    gpt_partition = image_gpt_partition,
)
image_buck1_dict = structs.to_dict(image_buck1)

image_buck2 = struct(
    layer = layer_buck2.new,
    layer_from_package = layer_buck2.from_package,
    genrule_layer = layer_buck2.genrule,
)
image_buck2_dict = structs.to_dict(image_buck2)

image = struct(**({
    key: image_buck2_dict[key] if key in image_buck2_dict else image_buck1_dict[key]
    for key in image_buck1_dict
} if use_buck2_macros() else image_buck1_dict))
