# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl:oss_shim.bzl", "kernel_get")
load("//antlir/bzl:shape.bzl", "shape")
load(":kernel.shape.bzl", "kernel_artifacts_t", "kernel_t")

def normalize_kernel(kernel):
    if types.is_string(kernel):
        return kernel_get.get(kernel)

    # Convert from a struct kernel struct format
    # into a kernel shape instance.  Note, if the provided `kernel` attr
    # is already a shape instance, this just makes another one. Wasteful, yes
    # but we don't have an `is_shape` mechanism yet to avoid something like
    # this.
    return shape.new(
        kernel_t,
        uname = kernel.uname,
        artifacts = shape.new(
            kernel_artifacts_t,
            devel = kernel.artifacts.devel,
            headers = kernel.artifacts.headers,
            modules = kernel.artifacts.modules,
            vmlinuz = kernel.artifacts.vmlinuz,
        ),
    )
