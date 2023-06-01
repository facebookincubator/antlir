# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":image_diff_test.bzl", "image_diff_test")
load(":image_rpms_test.bzl", "image_test_rpm_integrity", "image_test_rpm_names")
load(":image_test.bzl", "image_cpp_test", "image_python_test", "image_rust_test", "image_sh_test")

antlir2_image_test = struct(
    unittest = struct(
        cpp = image_cpp_test,
        python = image_python_test,
        rust = image_rust_test,
        sh = image_sh_test,
    ),
    diff_test = image_diff_test,
    rpms = struct(
        names = image_test_rpm_names,
        integrity = image_test_rpm_integrity,
    ),
)
