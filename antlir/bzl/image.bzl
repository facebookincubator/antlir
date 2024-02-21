# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"This provides a more friendly UI to the image_* macros."

load(":constants.bzl", "new_nevra")
load(":image_genrule_layer.bzl", "image_genrule_layer")
load(":image_gpt.bzl", "image_gpt", "image_gpt_partition")
load(":image_layer.bzl", "image_layer")
load(":image_test_rpm_names.bzl", "image_test_rpm_names")

image = struct(
    genrule_layer = image_genrule_layer,
    layer = image_layer,
    opts = struct,
    rpm = struct(nevra = new_nevra),
    test_rpm_names = image_test_rpm_names,
    gpt = image_gpt,
    gpt_partition = image_gpt_partition,
)
