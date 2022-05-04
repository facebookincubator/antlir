# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:kernel_shim.bzl", "kernels")

def normalize_kernel(kernel):
    if types.is_string(kernel):
        return kernels.get(kernel)

    return kernel
