# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

kernel_artifacts_t = shape.shape(
    vmlinuz = shape.target(),
    # devel and modules may not exist, such as in the case of a vmlinuz with
    # all necessary features compiled with =y
    devel = shape.target(optional = True),
    modules = shape.target(optional = True),
)

kernel_t = shape.shape(
    uname = str,
    artifacts = shape.field(kernel_artifacts_t),
)
